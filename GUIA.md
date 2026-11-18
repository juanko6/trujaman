# Whisper en local: voz a texto en el Mac

Guía desde cero para transcribir voz a texto sin internet ni APIs de pago, y dejarlo
listo para enchufarlo a cualquier app.

```
trujaman/
├── README.md          requisitos, puesta en marcha y resumen
├── GUIA.md            esto
├── modelos/           los modelos de Whisper (.bin)
├── audio/             grabaciones y sus .txt
└── scripts/
    ├── comun.sh       configuración: modelo, idioma, micro, puerto
    ├── grabar.sh      micro → WAV
    ├── transcribir.sh audio → texto
    ├── dictar.sh      grabar + transcribir + portapapeles
    ├── hablar.sh      texto → altavoces
    ├── interprete.sh  hablas en español, oyes inglés frase a frase
    ├── simultaneo.py  (lo usa el intérprete) el motor: traduce mientras hablas
    ├── locutor/       (lo usa el intérprete) voz siempre cargada, en Swift
    ├── en-vivo.sh     transcripción mientras hablas
    ├── servidor.sh    interfaz web + API HTTP local para otras apps
    ├── ui.py          (lo usa el servidor) backend de la interfaz
    └── ui.html        (lo usa el servidor) la página
```

---

## 1. Qué es cada pieza

- **Whisper** es el modelo de reconocimiento de voz de OpenAI. Es abierto: el modelo se
  descarga una vez y funciona sin conexión.
- **whisper.cpp** es la versión en C++ que lo ejecuta. En Mac usa la GPU (Metal) y no
  necesita Python. Trae varios programas: `whisper-cli` (transcribe ficheros),
  `whisper-server` (API HTTP) y `whisper-stream` (en directo).
- **ffmpeg** convierte cualquier audio al formato que pide Whisper: WAV a 16 kHz en mono.
  También graba del micrófono.

> Whisper **transcribe** en el idioma en que hablas. Su opción `translate` solo traduce
> *al inglés*. Para pasar voz a texto en español basta con transcribir.

## 2. Instalar

```bash
brew install whisper-cpp ffmpeg
```

Comprobar:

```bash
whisper-cli --version
```

## 3. Elegir y descargar un modelo

Los modelos vienen en formato `ggml`, desde el repositorio oficial
`huggingface.co/ggerganov/whisper.cpp`. Los `-q5_0` están cuantizados: ocupan
bastante menos y apenas se nota en la calidad.

| Modelo | Tamaño | Para qué |
|---|---|---|
| `ggml-base.bin` | 142 MB | Pruebas en máquinas flojas; en español falla bastante |
| `ggml-small.bin` | 466 MB | Aceptable, se come alguna palabra |
| **`ggml-large-v3-turbo-q5_0.bin`** | **547 MB** | **El recomendado: casi la calidad de large-v3 y muy rápido** |
| `ggml-large-v3.bin` | 2,9 GB | Máxima precisión, bastante más lento |

`scripts/doctor.sh --arreglar` descarga los dos modelos que usan los scripts. A mano, el recomendado:

```bash
cd modelos
curl -L -o ggml-large-v3-turbo-q5_0.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin
```

Para usar otro, se baja igual cambiando el nombre y se indica con `MODELO=...`.

## 4. Primera prueba (sin micrófono)

macOS puede generar una voz en español para probar:

```bash
say -v Mónica -o audio/prueba.aiff "Hola, esto es una prueba de transcripción en local."
scripts/transcribir.sh audio/prueba.aiff
```

Resultado medido en este Mac (M3 Max), con 13 s de audio:

| Modelo | Tiempo | Resultado |
|---|---|---|
| large-v3-turbo q5 | 1,5 s | Perfecto salvo "Whisper" → "Visper" |
| small | 0,6 s | Además, "Quasar y hay que" → "Quasaria y que" |

## 5. Con tu voz

La primera vez macOS pide permiso de micrófono para la Terminal: *Ajustes del Sistema →
Privacidad y seguridad → Micrófono*.

```bash
scripts/grabar.sh --lista      # ver micrófonos; el número va en MICRO=":N"
scripts/dictar.sh 10           # graba 10 s, transcribe y copia al portapapeles
scripts/dictar.sh              # graba hasta que pulses q
```

Transcribir notas de voz o vídeos que ya tengas (m4a del iPhone, mp3, mp4…):

```bash
scripts/transcribir.sh ~/Downloads/nota.m4a
```

El texto queda también en `nota.txt`, junto al audio.

En directo, mientras hablas (es menos preciso porque va por trozos):

```bash
scripts/en-vivo.sh
```

## 6. Ajustes

Todo se cambia con variables, sin tocar los scripts:

```bash
IDIOMA=en scripts/transcribir.sh talk.mp3          # otro idioma (o "auto")
MODELO=modelos/ggml-small.bin scripts/dictar.sh    # otro modelo
VOCABULARIO="Kubernetes, LinkedIn, PostgreSQL" scripts/dictar.sh
```

`VOCABULARIO` le da a Whisper una pista con los nombres propios y términos técnicos
que dices. Ayuda, pero no lo arregla todo.

### Traducir al inglés

Whisper sabe traducir de cualquier idioma **al inglés** (no a otros idiomas). Basta con
añadir `INGLES=1`:

```bash
INGLES=1 scripts/dictar.sh 10
INGLES=1 scripts/transcribir.sh nota.m4a     # deja nota.en.txt junto a nota.txt
```

El modelo turbo **no traduce**: con la opción de traducir devuelve el texto en español,
porque solo se entrenó para transcribir. Por eso `INGLES=1` cambia solo a
`ggml-medium-q5_0.bin` (514 MB), que se baja igual que el otro:

```bash
cd modelos
curl -L -o ggml-medium-q5_0.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin
```

Con el audio de prueba:

| Modelo | Traducción |
|---|---|
| large-v3-turbo | No traduce: devuelve el español |
| small | "…the launch of Quasaria and to review the budget of October." |
| **medium q5** | "…a meeting about the launch of Quasar and you have to review the October budget." (1 s) |

### Leerlo en alto

`hablar.sh` lee un texto por los altavoces con las voces de macOS (`say`). Con
`HABLAR=1`, `dictar.sh` lee el resultado al terminar. Así queda la cadena completa: hablas
en español y oyes la traducción en inglés:

```bash
INGLES=1 HABLAR=1 scripts/dictar.sh 10
scripts/hablar.sh "Hello there"           # un texto
INGLES=1 scripts/hablar.sh audio/prueba.en.txt   # un fichero
```

La voz se elige sola según el idioma de salida: Samantha en inglés y Mónica en
español. Se cambia con `VOZ=Daniel` (inglés británico) y la velocidad con
`VELOCIDAD=160` (palabras por minuto). `say -v '?'` lista las voces instaladas.

Las voces que vienen de serie suenan algo robóticas. Las versiones *Mejorada* y *Premium*
suenan mucho más naturales, se descargan gratis y siguen funcionando sin conexión: *Ajustes del Sistema →
Accesibilidad → Contenido leído → Voz del sistema → Gestionar voces…* → English →
por ejemplo "Ava (Premium)" o "Zoe (Premium)". Después: `VOZ="Ava (Premium)"`.

### En tiempo real: intérprete

Hablas en español y oyes el inglés:

```bash
AURICULARES=1 scripts/interprete.sh    # con cascos: traduce MIENTRAS hablas
scripts/interprete.sh                  # con altavoces: frase a frase, en cada pausa
SIN_VOZ=1 scripts/interprete.sh        # solo subtítulos (mientras hablas)
```

La primera vez compila el locutor (`scripts/locutor/`), que tarda unos segundos.

**Con cascos es simultáneo.** No espera a que acabes la frase: cada medio segundo
vuelve a traducir lo que llevas dicho y va diciendo lo que ya es firme. Con una frase
de prueba de 8,6 s:

| | Empieza a salir inglés | Última parte |
|---|---|---|
| Frase a frase | a los 9,6 s (todo de golpe) | 9,6 s |
| Simultáneo | **a los 2,3 s** | 9,6 s |

```
 2,3 s  Tomorrow morning I have to go
 3,4 s  to the boat to                    ← "banco": error de Whisper, no del troceo
 4,4 s  open a new account,
 5,0 s  and then I
 6,6 s  want to go to the market to
 7,5 s  buy fruit,
 9,6 s  because my parents come home for dinner on the weekend.
```

Va entre 1,5 y 2 s por detrás de tu voz. No puede bajar mucho más: para traducir una
palabra hay que haberla oído entera y algo de lo que viene después, porque el inglés
ordena la frase de otra manera.

**Cómo funciona** (`scripts/simultaneo.py`). Tres cosas corren a la vez:

```
hilo del micro   ffmpeg, abierto todo el rato
bucle principal  cada INTERVALO: frase en curso → whisper-server (modelo ya cargado)
                 compara con la pasada anterior → lo firme va al locutor
locutor          proceso aparte en Swift, con la voz ya cargada
```

- **Qué es "firme":** con el audio a medias, Whisper acierta el principio y se inventa
  el final. Se dan por buenas las palabras iniciales en las que coinciden dos pasadas
  seguidas, menos las `RESERVA` últimas. Lo dicho ya no se corrige.
- **Al callar** (`PAUSA`), se traduce la frase entera y se dice lo que faltaba.
- **El locutor no se queda atrás:** junta lo que tiene pendiente y lo dice de un tirón,
  habla más rápido cuanto más hay esperando y, si se acumula demasiado, se salta lo
  más viejo (avisa en pantalla).

**Con altavoces no puede ser simultáneo.** El micro oiría la traducción y la volvería a
traducir, así que se silencia mientras el locutor habla; por eso va frase a frase.

Ajustes:

| Variable | Por defecto | Para qué |
|---|---|---|
| `MICRO` | `:default` | El micro elegido en macOS. También por nombre: `MICRO=":MacBook Pro Microphone"` |
| `PAUSA` | `0.5` | Silencio que cierra la frase. Súbelo si te corta a medias |
| `UMBRAL` | `1.5%` | Volumen mínimo para contar como voz. Bájalo si no te oye; súbelo si hay ruido |
| `INTERVALO` | `0.5` | Cada cuánto retraduce. Menos = más rápido, pero más carga |
| `RESERVA` | `2` | Palabras finales que no se dan por firmes. `1` = más rápido, más errores; `3` = al revés |
| `MIN_TROZO` | `3` | Palabras mínimas para decir un trozo sin coma ni punto |
| `VOZ` | `Samantha` | Cualquier voz en inglés; las *Premium* suenan mucho mejor |
| `VOCABULARIO` | | Nombres propios y términos que falla |

Al conectar unos cascos Bluetooth, macOS suele pasar también el micro a los cascos, y
entonces el sonido baja de calidad. Para hablar por el micro del Mac y oír por los
cascos: `MICRO=":MacBook Pro Microphone" AURICULARES=1 scripts/interprete.sh`.

Si no traduce nada, el script avisa a los 4 s de si el micro entrega silencio o te oye
por debajo del umbral.

Para probar sin micro: `FICHERO=audio.wav MEDIR=1 SIN_VOZ=1 scripts/interprete.sh`
(wav a 16 kHz mono; enseña a qué segundo sale cada trozo).

> No se usa `whisper-stream`: en su modo por pausas vuelve a traducir los últimos 30 s
> de audio cada vez, así que repite frases. `en-vivo.sh` sí lo usa, pero solo para
> subtítulos en el mismo idioma.

Para traducir a otros idiomas, Whisper no basta: hay que transcribir y pasarle el
texto a un LLM, local o por API.

## 7. La interfaz web y el servidor para otras apps

```bash
scripts/servidor.sh          # abre http://127.0.0.1:8178
```

Hace dos cosas en la misma dirección: sirve la interfaz (explicada en el
[README](README.md#uso-la-interfaz)) y da servicio por HTTP a cualquier otra app. Con
`NO_ABRIR=1` no abre el navegador.

Se apaga solo unos 6 s después de cerrar la última pestaña (la página mantiene una
conexión abierta y el servidor nota cuándo desaparece), o al pulsar *Cerrar*. Con
`NO_ABRIR=1` o `NO_CERRAR=1` no se apaga solo: es lo que hace falta para usarlo como API.

Por dentro, `ui.py` arranca dos `whisper-server`, uno con cada modelo, y deja los dos
cargados. Medido en este Mac: la página responde a los 0,09 s y los dos modelos están
listos a los 3,7 s. Para escuchar, lanza `simultaneo.py` con `EVENTOS=1`, que en vez de
escribir texto suelta una línea JSON por cada cosa que pasa (nivel del micro, texto
provisional, trozo firme, fin de frase), y la página las recibe por Server-Sent Events.
Los dispositivos los lista `locutor --dispositivos` cada 2 s.

### La API

Cualquier app le manda el audio y recibe el texto. Acepta cualquier formato:

```bash
curl -F file=@audio/prueba.aiff -F response_format=json http://127.0.0.1:8178/inference
# {"text":" Hola, esto es una prueba de transcripción en local con Visper. ..."}
```

Para traducir al inglés, la misma ruta con `?ingles=1` (usa el modelo medium, que ya
está cargado):

```bash
curl -F file=@nota.m4a -F response_format=text "http://127.0.0.1:8178/inference?ingles=1"
```

Con `INGLES=1 scripts/servidor.sh`, `/inference` traduce sin tener que pedirlo.

Parámetros útiles del formulario: `response_format` (`json`, `text`, `srt`, `vtt`,
`verbose_json`), `language`, `prompt` (el vocabulario) y `temperature`.

Ejemplo en Go:

```go
func Transcribir(ruta string) (string, error) {
	f, err := os.Open(ruta)
	if err != nil {
		return "", err
	}
	defer f.Close()

	var cuerpo bytes.Buffer
	w := multipart.NewWriter(&cuerpo)
	parte, _ := w.CreateFormFile("file", filepath.Base(ruta))
	io.Copy(parte, f)
	w.WriteField("response_format", "json")
	w.Close()

	resp, err := http.Post("http://127.0.0.1:8178/inference", w.FormDataContentType(), &cuerpo)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	var r struct{ Text string `json:"text"` }
	if err := json.NewDecoder(resp.Body).Decode(&r); err != nil {
		return "", err
	}
	return strings.TrimSpace(r.Text), nil
}
```

Desde una web, lo mismo con `fetch` y un `FormData` con el blob que graba
`MediaRecorder`. En ese caso habría que añadir CORS o servir la web desde el mismo origen.

## 8. Llevarlo a una app de verdad

- **App de escritorio o backend propio:** lo más sencillo es el servidor del paso 7,
  como un proceso aparte, igual que el servidor MLX.
- **Dentro del binario, sin servidor:** whisper.cpp tiene bindings para Go
  (`github.com/ggerganov/whisper.cpp/bindings/go`, que necesita cgo), Swift, Python y
  Node. El modelo `.bin` va con la app o se descarga en el primer arranque.
- **iPhone o Mac nativo:** whisper.cpp también compila para iOS. Con Swift en macOS/iOS 26,
  el `SpeechAnalyzer` de Apple evita llevar un modelo encima.
- **Servidor en la nube sin GPU:** large-v3-turbo funciona en CPU, pero va lento. Ahí
  conviene `small`, o `faster-whisper` en Linux.

## 9. Problemas típicos

| Síntoma | Causa |
|---|---|
| Sale "Gracias por ver el video" u otra frase que no has dicho | La grabación está en silencio: Whisper se inventa frases con el audio vacío. Falta el permiso de micrófono para la app que lanza el script (Terminal, Claude, VS Code…), o el micro elegido no es el bueno (`grabar.sh --lista`, `MICRO=":nombre"`). `grabar.sh` ya avisa cuando ocurre |
| Texto repetido en bucle o inventado | Silencio largo o ruido; recorta los silencios del principio y del final |
| Falla nombres propios | Usa `VOCABULARIO=...` |
| Lento | Estás usando `large-v3` sin cuantizar; pasa a `large-v3-turbo-q5_0` |
