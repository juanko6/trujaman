#!/usr/bin/env python3
"""Interfaz web local de trujamán. La lanza servidor.sh.

Sirve una página (ui.html) en http://127.0.0.1:PUERTO y hace de intermediario entre
el navegador y lo que ya había:

  - dos whisper-server con el modelo cargado, uno transcribe y otro traduce;
  - simultaneo.py, que escucha el micro y va soltando eventos (uno por sesión);
  - locutor, que dice el texto y sabe qué dispositivos de audio hay.

El navegador no toca audio: solo manda órdenes (POST /api/…) y recibe lo que pasa
en directo (GET /api/eventos, Server-Sent Events).

También mantiene la API de siempre para otras apps:
    curl -F file=@nota.m4a http://127.0.0.1:8178/inference           transcribe
    curl -F file=@nota.m4a "http://127.0.0.1:8178/inference?ingles=1"  traduce

Solo usa la biblioteca estándar. Escucha únicamente en 127.0.0.1.
"""
import json
import os
import queue
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

AQUI = os.path.dirname(os.path.abspath(__file__))
RAIZ = os.path.dirname(AQUI)
ENT = os.environ.get

PUERTO = int(ENT("PUERTO", "8178"))
IDIOMA = ENT("IDIOMA", "es")
INGLES = bool(ENT("INGLES"))                 # /inference traduce por defecto
LOCUTOR = ENT("LOCUTOR", os.path.join(AQUI, "locutor", "locutor"))
FICHERO = ENT("FICHERO", "")                 # pruebas: el motor lee este wav en vez del micro
MODELOS = {
    "transcribir": ENT("MODELO_TRANSCRIBIR", os.path.join(RAIZ, "modelos", "ggml-large-v3-turbo-q5_0.bin")),
    "traducir": ENT("MODELO_TRADUCIR", os.path.join(RAIZ, "modelos", "ggml-medium-q5_0.bin")),
}
MAX_FICHERO = 2 * 1024 ** 3                  # 2 GB

tmp = tempfile.mkdtemp(prefix="trujaman.")
cerrojo = threading.Lock()
oyentes = []                                 # una cola por pestaña conectada a /api/eventos
estado = {
    "modelos": {"transcribir": "cargando", "traducir": "cargando"},
    "dispositivos": {"entradas": [], "salidas": [], "voces": []},
    "escuchando": False,
    "prueba": bool(FICHERO),
}
servidores = {}                              # modo → {"url":…, "proceso":…}
sesion = None                                # el simultaneo.py en marcha, si lo hay


def registro(texto):
    print(texto, file=sys.stderr, flush=True)


def difundir(evento):
    """Manda un evento a todas las pestañas abiertas."""
    with cerrojo:
        for cola in oyentes:
            cola.put(evento)


# --- Whisper ----------------------------------------------------------------
def puerto_libre():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def multiparte(campos, fichero=None):
    """Cuerpo multipart/form-data. fichero = (nombre, bytes)."""
    limite = uuid.uuid4().hex
    cuerpo = b""
    for nombre, valor in campos.items():
        cuerpo += (f'--{limite}\r\nContent-Disposition: form-data; name="{nombre}"\r\n\r\n'
                   f"{valor}\r\n").encode()
    if fichero:
        nombre = fichero[0].replace('"', "").replace("\r", "").replace("\n", "") or "audio"
        cuerpo += (f'--{limite}\r\nContent-Disposition: form-data; name="file"; filename="{nombre}"\r\n'
                   "Content-Type: application/octet-stream\r\n\r\n").encode() + fichero[1] + b"\r\n"
    cuerpo += f"--{limite}--\r\n".encode()
    return cuerpo, f"multipart/form-data; boundary={limite}"


def a_whisper(modo, cuerpo, tipo, espera=3600):
    peticion = urllib.request.Request(servidores[modo]["url"] + "/inference", data=cuerpo,
                                      headers={"Content-Type": tipo})
    with urllib.request.urlopen(peticion, timeout=espera) as r:
        return r.read(), r.headers.get("Content-Type", "text/plain")


def arrancar_whisper(modo):
    """Lanza un whisper-server y avisa cuando el modelo está cargado."""
    if not os.path.exists(MODELOS[modo]):
        estado["modelos"][modo] = "falta"
        difundir({"tipo": "modelos", "modelos": estado["modelos"]})
        registro(f"  ✗ falta el modelo para {modo}: scripts/doctor.sh --arreglar")
        return
    puerto = puerto_libre()
    orden = ["whisper-server", "-m", MODELOS[modo], "-l", IDIOMA,
             "--host", "127.0.0.1", "--port", str(puerto), "--convert"]
    if modo == "traducir":
        orden.append("--translate")
    proceso = subprocess.Popen(orden, cwd=tmp, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    servidores[modo] = {"url": f"http://127.0.0.1:{puerto}", "proceso": proceso}
    inicio = time.time()
    while True:
        try:
            urllib.request.urlopen(servidores[modo]["url"] + "/", timeout=1)
            break
        except Exception:
            if proceso.poll() is not None:
                estado["modelos"][modo] = "error"
                difundir({"tipo": "modelos", "modelos": estado["modelos"]})
                registro(f"  ✗ whisper-server ({modo}) no arranca")
                return
            time.sleep(0.1)
    estado["modelos"][modo] = "listo"
    difundir({"tipo": "modelos", "modelos": estado["modelos"]})
    registro(f"  ✓ modelo para {modo} listo en {time.time() - inicio:.1f} s")


# --- Dispositivos -----------------------------------------------------------
def vigilar_dispositivos():
    """Cada 2 s mira qué micros, salidas y voces hay; avisa solo si cambia algo."""
    avisado = False
    while True:
        try:
            salida = subprocess.run([LOCUTOR, "--dispositivos"], capture_output=True, timeout=10).stdout
            actuales = json.loads(salida)
        except Exception:
            if not avisado:
                avisado = True
                registro("  ✗ el locutor no responde: scripts/doctor.sh --arreglar")
                difundir({"tipo": "aviso", "texto": "No puedo detectar los dispositivos de audio. "
                                                    "Lanza scripts/doctor.sh --arreglar."})
            actuales = None
        if actuales and actuales != estado["dispositivos"]:
            estado["dispositivos"] = actuales
            difundir({"tipo": "dispositivos", **actuales})
        time.sleep(2)


# --- Sesión de escucha --------------------------------------------------------
def parar():
    global sesion
    proceso, sesion = sesion, None
    if proceso and proceso.poll() is None:
        proceso.terminate()
        try:
            proceso.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proceso.kill()


def empezar(orden):
    """Lanza simultaneo.py con lo elegido en la página. Devuelve un error o None."""
    global sesion
    modo = "transcribir" if orden.get("modo") == "transcribir" else "traducir"
    if estado["modelos"][modo] != "listo":
        return "El modelo todavía se está cargando." if estado["modelos"][modo] == "cargando" \
            else "Falta el modelo: lanza scripts/doctor.sh --arreglar."
    parar()

    hablar = bool(orden.get("hablar")) and modo == "traducir"
    entorno = dict(os.environ)
    for sobra in ("AURICULARES", "SIN_VOZ", "SIMULTANEO", "MEDIR", "FICHERO"):
        entorno.pop(sobra, None)
    entorno.update(
        SERVIDOR=servidores[modo]["url"], EVENTOS="1", IDIOMA=IDIOMA, LOCUTOR=LOCUTOR,
        TRADUCIR="1" if modo == "traducir" else "0",
        MICRO=":" + (str(orden.get("micro") or "default")),
        SALIDA=str(orden.get("salida") or ""),
        VOZ=str(orden.get("voz") or "Samantha"))
    if orden.get("cascos"):
        entorno["AURICULARES"] = "1"
    if not hablar:
        entorno["SIN_VOZ"] = "1"
    if FICHERO:
        entorno["FICHERO"] = FICHERO

    proceso = subprocess.Popen([sys.executable, os.path.join(AQUI, "simultaneo.py")], env=entorno,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    sesion = proceso
    estado["escuchando"] = True

    def reenviar():
        for linea in proceso.stdout:
            try:
                difundir(json.loads(linea))
            except ValueError:
                pass
        proceso.wait()
        if sesion is proceso:                     # acabó sola (no la paró nadie)
            estado["escuchando"] = False
            difundir({"tipo": "parado"})

    threading.Thread(target=reenviar, daemon=True).start()
    return None


# --- HTTP -------------------------------------------------------------------
class Peticion(BaseHTTPRequestHandler):
    server_version = "trujaman"

    def log_message(self, *_):
        pass

    def permitido(self):
        """Solo esta misma página: otra web abierta en el navegador no puede dar órdenes."""
        propios = {f"127.0.0.1:{PUERTO}", f"localhost:{PUERTO}"}
        if self.headers.get("Host", "") not in propios:
            return False
        origen = self.headers.get("Origin")
        return origen is None or origen in {f"http://{p}" for p in propios}

    def responder(self, codigo, cuerpo, tipo="application/json; charset=utf-8"):
        if isinstance(cuerpo, (dict, list)):
            cuerpo = json.dumps(cuerpo, ensure_ascii=False).encode()
        elif isinstance(cuerpo, str):
            cuerpo = cuerpo.encode()
        self.send_response(codigo)
        self.send_header("Content-Type", tipo)
        self.send_header("Content-Length", str(len(cuerpo)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(cuerpo)

    def cuerpo(self):
        largo = int(self.headers.get("Content-Length") or 0)
        if largo > MAX_FICHERO:
            return None
        return self.rfile.read(largo)

    def do_GET(self):
        if not self.permitido():
            return self.responder(403, {"error": "origen no permitido"})
        ruta = urllib.parse.urlparse(self.path).path
        if ruta in ("/", "/index.html"):
            with open(os.path.join(AQUI, "ui.html"), "rb") as f:
                return self.responder(200, f.read(), "text/html; charset=utf-8")
        if ruta == "/api/estado":
            return self.responder(200, estado)
        if ruta == "/api/eventos":
            return self.eventos()
        self.responder(404, {"error": "no existe"})

    def eventos(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        cola = queue.Queue()
        with cerrojo:
            oyentes.append(cola)
        try:
            cola.put({"tipo": "estado", **estado})       # foto inicial
            while True:
                try:
                    evento = cola.get(timeout=15)
                    linea = "data: " + json.dumps(evento, ensure_ascii=False) + "\n\n"
                except queue.Empty:
                    linea = ": sigo aquí\n\n"                # para que no se corte por inactividad
                self.wfile.write(linea.encode())
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, OSError):
            pass
        finally:
            with cerrojo:
                oyentes.remove(cola)

    def do_POST(self):
        if not self.permitido():
            return self.responder(403, {"error": "origen no permitido"})
        url = urllib.parse.urlparse(self.path)
        consulta = urllib.parse.parse_qs(url.query)
        ingles = consulta.get("ingles", ["1" if INGLES else "0"])[0] == "1"
        modo = "traducir" if ingles else "transcribir"

        if url.path == "/api/parar":
            parar()
            estado["escuchando"] = False
            difundir({"tipo": "parado"})
            return self.responder(200, {"ok": True})

        datos = self.cuerpo()
        if datos is None:
            return self.responder(413, {"error": "El fichero pasa de 2 GB."})

        if url.path == "/api/empezar":
            try:
                orden = json.loads(datos or b"{}")
            except ValueError:
                return self.responder(400, {"error": "petición mal formada"})
            error = empezar(orden)
            return self.responder(409, {"error": error}) if error else self.responder(200, {"ok": True})

        if url.path in ("/api/archivo", "/inference"):
            if estado["modelos"][modo] != "listo":
                return self.responder(503, {"error": "El modelo todavía no está listo."})
            try:
                if url.path == "/inference":                 # API de siempre: se pasa tal cual
                    respuesta, tipo = a_whisper(modo, datos, self.headers.get("Content-Type", ""))
                    return self.responder(200, respuesta, tipo)
                inicio = time.time()
                nombre = consulta.get("nombre", ["audio"])[0]
                cuerpo, tipo = multiparte({"response_format": "text", "beam_size": "5"}, (nombre, datos))
                respuesta, _ = a_whisper(modo, cuerpo, tipo)
                texto = re.sub(r"\[[^\]]*\]|\([^)]*\)", " ", respuesta.decode("utf-8", "replace"))  # [BLANK_AUDIO], (música)…
                lineas = [l.strip() for l in texto.splitlines() if l.strip()]
                return self.responder(200, {"texto": " ".join(lineas),
                                            "segundos": round(time.time() - inicio, 1)})
            except urllib.error.HTTPError as error:
                return self.responder(422, {"error": "No he podido leer ese fichero. ¿Es un audio o un vídeo?"})
            except Exception as error:
                return self.responder(502, {"error": f"Whisper no responde: {error}"})

        self.responder(404, {"error": "no existe"})


def salir(*_):
    parar()
    for servidor in servidores.values():
        try:
            servidor["proceso"].terminate()
        except Exception:
            pass
    shutil.rmtree(tmp, ignore_errors=True)
    os._exit(0)


def principal():
    direccion = f"http://127.0.0.1:{PUERTO}"
    try:
        web = ThreadingHTTPServer(("127.0.0.1", PUERTO), Peticion)
    except OSError:
        registro(f"El puerto {PUERTO} ya está en uso: ¿trujamán ya está abierto? Abro {direccion}")
        if not ENT("NO_ABRIR"):
            subprocess.run(["open", direccion])
        sys.exit(1)
    web.daemon_threads = True
    signal.signal(signal.SIGINT, salir)
    signal.signal(signal.SIGTERM, salir)

    registro(f"trujamán en {direccion}   (Ctrl+C para cerrar)")
    for modo in MODELOS:
        threading.Thread(target=arrancar_whisper, args=(modo,), daemon=True).start()
    threading.Thread(target=vigilar_dispositivos, daemon=True).start()
    if not ENT("NO_ABRIR"):
        subprocess.Popen(["open", direccion])
    try:
        web.serve_forever()
    finally:
        salir()


if __name__ == "__main__":
    principal()
