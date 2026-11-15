#!/usr/bin/env python3
"""Motor del intérprete: traduce al inglés mientras hablas, sin esperar a que acabes.

Lo lanza interprete.sh, que le pasa la configuración por variables de entorno.

Tres cosas corren a la vez:
  - un hilo que lee el micro (ffmpeg) sin parar;
  - el bucle principal, que cada INTERVALO segundos manda a whisper-server todo el
    audio de la frase en curso y decide qué parte de la traducción ya es firme;
  - el locutor, un proceso aparte que va diciendo los trozos firmes.

Qué es "firme": Whisper, con el audio a medias, acierta el principio y se inventa
el final. Así que se comparan dos pasadas seguidas: las palabras iniciales en las
que coinciden (menos las RESERVA últimas, por prudencia) se dan por buenas y se
dicen ya. Lo dicho no se corrige; cuando callas, se traduce la frase entera y se
dice lo que faltaba.

Prueba sin micro:  FICHERO=audio.wav python3 simultaneo.py   (wav 16 kHz mono)
"""
import array
import io
import math
import os
import queue
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import uuid
import wave

# --- Configuración (variables de entorno) ---------------------------------
ENT = os.environ.get
MODELO = ENT("MODELO", "")
IDIOMA = ENT("IDIOMA", "es")
MICRO = ENT("MICRO", ":default")
VOZ = ENT("VOZ", "Samantha")
LOCUTOR = ENT("LOCUTOR", "")
PUERTO = int(ENT("PUERTO_INT", "8187"))
VOCABULARIO = ENT("VOCABULARIO", "")
SIN_VOZ = bool(ENT("SIN_VOZ"))
AURICULARES = bool(ENT("AURICULARES"))
FICHERO = ENT("FICHERO", "")             # para pruebas: lee un wav en vez del micro
MEDIR = bool(ENT("MEDIR"))               # enseña a qué segundo sale cada trozo

PAUSA = float(ENT("PAUSA", "0.5"))       # silencio que cierra una frase
UMBRAL = float(ENT("UMBRAL", "1.5%").rstrip("%")) / 100   # volumen mínimo para contar como voz
INTERVALO = float(ENT("INTERVALO", "0.5"))  # cada cuánto se retraduce la frase en curso
RESERVA = int(ENT("RESERVA", "2"))       # palabras finales que no se dan por firmes
MIN_TROZO = int(ENT("MIN_TROZO", "3"))   # palabras mínimas para decir un trozo sin puntuación
MAX_FRASE = float(ENT("MAX_FRASE", "20"))  # segundos; una frase más larga se corta aquí
# Con altavoces el micro se silencia mientras habla el locutor, así que hablar a la
# vez que él no sirve: se vuelve a frase por frase.
SIMULTANEO = AURICULARES or SIN_VOZ or bool(ENT("SIMULTANEO"))

FRECUENCIA = 16000
TRAMA = int(FRECUENCIA * 0.03) * 2       # 30 ms en bytes (16 bits)
SEG = FRECUENCIA * 2                     # bytes por segundo
MIN_AUDIO = 1.0                          # no se traduce una frase hasta tener esto de audio
PREVIO = 0.3                             # audio anterior al arranque de la voz que se conserva

INVENTADAS = {"", ".", "you", "thank you.", "thanks for watching!", "thank you for watching.",
              "thank you very much.", "bye.", "subtitles by the amara.org community"}

URL = f"http://127.0.0.1:{PUERTO}/inference"
procesos = []
tmp = tempfile.mkdtemp(prefix="interprete.")
hablando = os.path.join(tmp, "hablando")
t0 = time.time()


def aviso(texto):
    print(texto, file=sys.stderr, flush=True)


def salir(*_):
    for p in procesos:
        try:
            p.terminate()
        except Exception:
            pass
    shutil.rmtree(tmp, ignore_errors=True)
    os._exit(0)


signal.signal(signal.SIGINT, salir)
signal.signal(signal.SIGTERM, salir)


# --- Whisper ----------------------------------------------------------------
def traducir(pcm, parcial):
    """Manda audio PCM al servidor y devuelve la traducción limpia."""
    wav = io.BytesIO()
    with wave.open(wav, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(FRECUENCIA)
        w.writeframes(pcm)

    campos = {"response_format": "text"}
    if parcial:      # sin reintentos: más rápido, y un parcial malo se descarta solo
        campos.update(temperature="0", temperature_inc="0")
    if VOCABULARIO:
        campos["prompt"] = VOCABULARIO

    limite = uuid.uuid4().hex
    cuerpo = b""
    for nombre, valor in campos.items():
        cuerpo += (f'--{limite}\r\nContent-Disposition: form-data; name="{nombre}"\r\n\r\n'
                   f"{valor}\r\n").encode()
    cuerpo += (f'--{limite}\r\nContent-Disposition: form-data; name="file"; filename="a.wav"\r\n'
               "Content-Type: audio/wav\r\n\r\n").encode() + wav.getvalue() + f"\r\n--{limite}--\r\n".encode()
    peticion = urllib.request.Request(
        URL, data=cuerpo, headers={"Content-Type": f"multipart/form-data; boundary={limite}"})
    with urllib.request.urlopen(peticion, timeout=30) as r:
        texto = r.read().decode("utf-8", "replace")

    texto = re.sub(r"\[[^\]]*\]|\([^)]*\)", " ", texto)       # [BLANK_AUDIO], (music)…
    texto = re.sub(r"\.\.\.+", "", texto)                     # los "…" de frase a medias
    return " ".join(texto.split())


def clave(palabra):
    """Para comparar palabras entre pasadas sin que estorben mayúsculas ni comas."""
    return re.sub(r"\W", "", palabra.lower())


def coinciden(a, b):
    """Cuántas palabras iniciales comparten dos traducciones."""
    n = 0
    for x, y in zip(a, b):
        if clave(x) != clave(y):
            break
        n += 1
    return n


# --- Salida -----------------------------------------------------------------
class Salida:
    """Pantalla + locutor."""

    def __init__(self):
        self.voz = None
        self.linea_abierta = False
        if not SIN_VOZ:
            marca = "" if AURICULARES else hablando
            self.voz = subprocess.Popen([LOCUTOR, VOZ, marca], stdin=subprocess.PIPE, text=True)
            procesos.append(self.voz)

    def decir(self, palabras):
        if not palabras:
            return
        trozo = " ".join(palabras)
        if MEDIR:
            print(f"  [{time.time() - t0:5.1f}s] {trozo}", flush=True)
        else:
            print(("  " if self.linea_abierta else "→ ") + trozo, end="", flush=True)
            self.linea_abierta = True
        if self.voz:
            try:
                self.voz.stdin.write(trozo + "\n")
                self.voz.stdin.flush()
            except BrokenPipeError:
                aviso("El locutor se ha cerrado.")
                salir()

    def fin_frase(self):
        if self.linea_abierta:
            print(flush=True)
            self.linea_abierta = False


# --- Micro ------------------------------------------------------------------
def leer_micro(cola):
    """Hilo: mete tramas de 30 ms en la cola. None = se acabó el audio."""
    if FICHERO:
        with wave.open(FICHERO, "rb") as w:
            datos = w.readframes(w.getnframes())
        datos += bytes(SEG * 2)                     # silencio final para cerrar la frase
        inicio = time.time()
        for i in range(0, len(datos), TRAMA):
            cola.put(datos[i:i + TRAMA])
            falta = inicio + (i + TRAMA) / SEG - time.time()   # a ritmo real
            if falta > 0:
                time.sleep(falta)
        cola.put(None)
        return

    ffmpeg = subprocess.Popen(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "avfoundation", "-i", MICRO,
         "-ar", str(FRECUENCIA), "-ac", "1", "-f", "s16le", "-"],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0)
    procesos.append(ffmpeg)
    while True:
        trama = ffmpeg.stdout.read(TRAMA)
        if not trama:
            break
        if not AURICULARES and os.path.exists(hablando):
            trama = bytes(len(trama))               # no oírse a sí mismo por los altavoces
        cola.put(trama)
    cola.put(None)


def volumen(trama):
    muestras = array.array("h", trama[:len(trama) // 2 * 2])
    if not muestras:
        return 0.0
    return math.sqrt(sum(m * m for m in muestras) / len(muestras)) / 32768


# --- Bucle principal --------------------------------------------------------
def principal():
    servidor = subprocess.Popen(
        ["whisper-server", "-m", MODELO, "-l", IDIOMA, "--translate",
         "--host", "127.0.0.1", "--port", str(PUERTO)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    procesos.append(servidor)
    salida = Salida()

    while True:                                     # esperar a que cargue el modelo
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PUERTO}/", timeout=1)
            break
        except Exception:
            if servidor.poll() is not None:
                aviso(f"whisper-server no arranca (¿puerto {PUERTO} ocupado o falta el modelo?).")
                salir()
            time.sleep(0.2)
    traducir(bytes(SEG), parcial=True)              # primera pasada en frío, para que no la pague la primera frase

    cola = queue.Queue()
    threading.Thread(target=leer_micro, args=(cola,), daemon=True).start()

    modo = "mientras hablas" if SIMULTANEO else "frase a frase (con AURICULARES=1, mientras hablas)"
    aviso(f"Escuchando por {'el micro del sistema' if MICRO == ':default' else MICRO.lstrip(':')}… habla en español; traduce {modo}. Ctrl+C para salir.")
    global t0
    t0 = time.time()

    previo = bytearray()        # últimos PREVIO s, por si arranca la voz
    frase = bytearray()         # audio de la frase en curso
    en_frase = False
    seguidas = 0                # tramas seguidas con voz (para arrancar)
    silencio = 0.0              # segundos callado dentro de la frase
    anterior = []               # traducción de la pasada anterior
    dichas = 0                  # palabras de esta frase ya enviadas al locutor
    pico, avisado = 0.0, False  # para avisar si el micro no entrega nada
    ultima_pasada = 0.0
    audio_en_pasada = 0

    def cerrar_frase():
        nonlocal en_frase, frase, anterior, dichas, silencio, seguidas
        final = traducir(bytes(frase), parcial=False).split()
        if not (dichas == 0 and " ".join(final).lower() in INVENTADAS):
            salida.decir(final[dichas:])
        salida.fin_frase()
        en_frase, frase, anterior, dichas, silencio, seguidas = False, bytearray(), [], 0, 0.0, 0

    while True:
        # 1. Recoger todo el audio que haya llegado.
        try:
            tramas = [cola.get(timeout=0.05)]
            while not cola.empty():
                tramas.append(cola.get_nowait())
        except queue.Empty:
            tramas = []

        for trama in tramas:
            if trama is None:
                if en_frase:
                    cerrar_frase()
                if FICHERO:
                    if salida.voz:                  # dejar que termine de hablar
                        salida.voz.stdin.close()
                        salida.voz.wait()
                    salir()
                aviso(f"Se ha cortado el micro ({MICRO}). Mira ./grabar.sh --lista y el permiso de micrófono.")
                salir()

            nivel = volumen(trama)
            hay_voz = nivel > UMBRAL
            pico = max(pico, nivel)
            if not en_frase:
                previo += trama
                del previo[:-int(PREVIO * SEG)]
                seguidas = seguidas + 1 if hay_voz else 0
                if seguidas >= 3:                   # 90 ms de voz: empieza una frase
                    en_frase, frase, silencio = True, bytearray(previo), 0.0
                    previo.clear()
                    ultima_pasada, audio_en_pasada = time.time(), 0
            else:
                frase += trama
                silencio = 0.0 if hay_voz else silencio + len(trama) / SEG

        if not avisado and time.time() - t0 > 4:
            avisado = True
            if pico < 0.0005:
                aviso(f"Ojo: el micro ({MICRO}) solo entrega silencio. Elige otro con MICRO=\":nombre\" "
                      "(./grabar.sh --lista) o revisa el permiso de micrófono.")
            elif pico < UMBRAL:
                aviso(f"Ojo: te oigo muy bajo (pico {pico:.1%}, umbral {UMBRAL:.1%}). "
                      "Acércate al micro o baja el umbral, por ejemplo UMBRAL=0.5%.")

        if not en_frase:
            continue

        # 2. ¿Se ha acabado la frase?
        if silencio >= PAUSA or len(frase) / SEG >= MAX_FRASE:
            cerrar_frase()
            continue

        # 3. Pasada parcial: retraducir lo que va de frase y decir lo que ya es firme.
        if not SIMULTANEO or silencio > 0.15:       # callado: mejor esperar al cierre
            continue
        if time.time() - ultima_pasada < INTERVALO or len(frase) / SEG < MIN_AUDIO:
            continue
        if len(frase) == audio_en_pasada:
            continue
        ultima_pasada, audio_en_pasada = time.time(), len(frase)
        actual = traducir(bytes(frase), parcial=True).split()

        firmes = max(0, coinciden(anterior, actual) - RESERVA)
        anterior = actual
        nuevas = actual[dichas:firmes]
        if not nuevas:
            continue
        # Cortar en la última puntuación si la hay; si no, esperar a tener un trozo decente.
        corte = max((i + 1 for i, p in enumerate(nuevas) if p[-1] in ",.;:?!"), default=0)
        if corte == 0 and len(nuevas) >= MIN_TROZO:
            corte = len(nuevas)
        if corte:
            salida.decir(nuevas[:corte])
            dichas += corte


if __name__ == "__main__":
    try:
        principal()
    except KeyboardInterrupt:
        pass
    finally:
        salir()
