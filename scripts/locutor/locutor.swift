// Locutor: lee trozos de texto por la entrada estándar (uno por línea) y los dice en alto.
// Se queda abierto con la voz cargada, así cada trozo empieza a sonar enseguida
// (`say` tarda casi un segundo en arrancar cada vez).
//
//   locutor [VOZ] [FICHERO_HABLANDO] [SALIDA]
//   locutor --dispositivos
//
// VOZ: nombre como sale en `say -v '?'` ("Samantha", "Daniel", "Ava (Premium)").
// FICHERO_HABLANDO: si se da (y no es ""), existe mientras habla y 0,4 s más, por el
// eco de la sala. El intérprete lo usa para no escucharse a sí mismo por los altavoces.
// SALIDA: nombre del dispositivo por el que hablar. Sin él, el predeterminado de macOS.
//
// --dispositivos imprime en JSON las entradas y salidas de audio y las voces
// instaladas; lo usa la interfaz web para rellenar sus desplegables.
//
// Lleva su propia cola para no quedarse atrás cuando llega texto más rápido de lo
// que puede decirlo:
//   - al acabar un trozo, junta todo lo que esté esperando y lo dice de un tirón;
//   - cuanto más hay esperando, más rápido habla;
//   - si hay demasiado, se salta lo más viejo (como los subtítulos en directo).
//
// Compilar: swiftc -O locutor.swift -o locutor
import AVFoundation
import CoreAudio
import Foundation

func aviso(_ texto: String) {
    FileHandle.standardError.write((texto + "\n").data(using: .utf8)!)
}

// --- Dispositivos de audio (CoreAudio) ----------------------------------------

func direccion(_ selector: AudioObjectPropertySelector,
               _ ambito: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector: selector, mScope: ambito, mElement: kAudioObjectPropertyElementMain)
}

func tamano(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector,
            _ ambito: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> UInt32 {
    var dir = direccion(selector, ambito)
    var tam: UInt32 = 0
    AudioObjectGetPropertyDataSize(id, &dir, 0, nil, &tam)
    return tam
}

func leer<T>(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector, _ valor: inout T) -> Bool {
    var dir = direccion(selector)
    var tam = UInt32(MemoryLayout<T>.size)
    return withUnsafeMutablePointer(to: &valor) {
        AudioObjectGetPropertyData(id, &dir, 0, nil, &tam, $0) == noErr
    }
}

struct Dispositivo {
    let id: AudioDeviceID
    let nombre: String
    let esEntrada: Bool
    let esSalida: Bool
    let integrado: Bool
}

func dispositivos() -> [Dispositivo] {
    let sistema = AudioObjectID(kAudioObjectSystemObject)
    let cuantos = Int(tamano(sistema, kAudioHardwarePropertyDevices)) / MemoryLayout<AudioDeviceID>.size
    guard cuantos > 0 else { return [] }
    var ids = [AudioDeviceID](repeating: 0, count: cuantos)
    var dir = direccion(kAudioHardwarePropertyDevices)
    var tam = UInt32(cuantos * MemoryLayout<AudioDeviceID>.size)
    guard AudioObjectGetPropertyData(sistema, &dir, 0, nil, &tam, &ids) == noErr else { return [] }

    return ids.compactMap { id in
        var nombre = "" as CFString
        guard leer(id, kAudioObjectPropertyName, &nombre) else { return nil }
        var transporte: UInt32 = 0
        _ = leer(id, kAudioDevicePropertyTransportType, &transporte)
        return Dispositivo(
            id: id,
            nombre: nombre as String,
            esEntrada: tamano(id, kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput) > 0,
            esSalida: tamano(id, kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput) > 0,
            integrado: transporte == kAudioDeviceTransportTypeBuiltIn)
    }
}

func predeterminado(_ selector: AudioObjectPropertySelector) -> AudioDeviceID {
    var id: AudioDeviceID = 0
    _ = leer(AudioObjectID(kAudioObjectSystemObject), selector, &id)
    return id
}

func nombreVisible(_ voz: AVSpeechSynthesisVoice) -> String {
    switch voz.quality {
    case .premium: return voz.name + " (Premium)"
    case .enhanced: return voz.name + " (Enhanced)"
    default: return voz.name
    }
}

func imprimirDispositivos() {
    let todos = dispositivos()
    let entrada = predeterminado(kAudioHardwarePropertyDefaultInputDevice)
    let salida = predeterminado(kAudioHardwarePropertyDefaultOutputDevice)
    func fila(_ d: Dispositivo, _ porDefecto: AudioDeviceID) -> [String: Any] {
        ["nombre": d.nombre, "predeterminado": d.id == porDefecto, "integrado": d.integrado]
    }
    var voces: [[String: Any]] = []
    for voz in AVSpeechSynthesisVoice.speechVoices() {
        let idioma = String(voz.language.prefix(2))
        guard idioma == "en" || idioma == "es" else { continue }
        if #available(macOS 14.0, *), voz.voiceTraits.contains(.isNoveltyVoice) { continue }
        voces.append(["nombre": nombreVisible(voz), "idioma": voz.language, "calidad": voz.quality.rawValue])
    }
    let todo: [String: Any] = [
        "entradas": todos.filter { $0.esEntrada }.map { fila($0, entrada) },
        "salidas": todos.filter { $0.esSalida }.map { fila($0, salida) },
        "voces": voces,
    ]
    let json = try! JSONSerialization.data(withJSONObject: todo, options: [.sortedKeys])
    FileHandle.standardOutput.write(json)
    FileHandle.standardOutput.write("\n".data(using: .utf8)!)
}

if CommandLine.arguments.contains("--dispositivos") {
    imprimirDispositivos()
    exit(0)
}

// --- Voz ----------------------------------------------------------------------

let args = CommandLine.arguments
let nombreVoz = args.count > 1 ? args[1] : "Samantha"
let ficheroHablando = args.count > 2 ? args[2] : ""
let nombreSalida = args.count > 3 ? args[3] : ""

let ritmoBase = AVSpeechUtteranceDefaultSpeechRate          // 0,5
let ritmoMaximo: Float = 0.62                               // más rápido deja de entenderse
let palabrasSinPrisa = 5                                    // hasta aquí, ritmo normal
let palabrasMaximas = 28                                    // más que esto esperando: se salta lo viejo

func buscarVoz(_ nombre: String) -> AVSpeechSynthesisVoice? {
    let voces = AVSpeechSynthesisVoice.speechVoices()
    let base = nombre.components(separatedBy: " (").first ?? nombre
    let minusculas = nombre.lowercased()
    let candidatas = voces.filter { $0.name == base }
    if minusculas.contains("premium"), let v = candidatas.first(where: { $0.quality == .premium }) { return v }
    if minusculas.contains("mejorada") || minusculas.contains("enhanced"),
       let v = candidatas.first(where: { $0.quality == .enhanced }) { return v }
    // Sin calificativo: la mejor calidad instalada con ese nombre.
    return candidatas.max { $0.quality.rawValue < $1.quality.rawValue }
}

func contarPalabras(_ trozos: [String]) -> Int {
    trozos.reduce(0) { $0 + $1.split(separator: " ").count }
}

/// Reproduce por un dispositivo concreto el audio que sintetiza AVSpeechSynthesizer.
/// Hace falta porque en macOS el sintetizador solo sabe hablar por la salida predeterminada.
final class Reproductor: @unchecked Sendable {
    let motor = AVAudioEngine()
    let nodo = AVAudioPlayerNode()
    var formato: AVAudioFormat?
    var conversor: AVAudioConverter?

    init?(dispositivo: AudioDeviceID) {
        motor.attach(nodo)
        do {
            try motor.outputNode.auAudioUnit.setDeviceID(dispositivo)
        } catch {
            return nil
        }
    }

    /// Encola un búfer; `alAcabar` se llama cuando ha terminado de sonar.
    func encolar(_ entrada: AVAudioPCMBuffer, alAcabar: @escaping @Sendable () -> Void) {
        // El mezclador quiere coma flotante; el sintetizador suele dar enteros de 16 bits.
        if formato == nil || conversor?.inputFormat != entrada.format {
            guard let destino = AVAudioFormat(standardFormatWithSampleRate: entrada.format.sampleRate,
                                              channels: entrada.format.channelCount) else { alAcabar(); return }
            formato = destino
            conversor = AVAudioConverter(from: entrada.format, to: destino)
            motor.stop()
            motor.connect(nodo, to: motor.mainMixerNode, format: destino)
            do { try motor.start() } catch { aviso("locutor: no puedo abrir la salida: \(error.localizedDescription)"); alAcabar(); return }
            nodo.play()
        }
        guard let destino = formato, let conversor = conversor,
              let salida = AVAudioPCMBuffer(pcmFormat: destino, frameCapacity: entrada.frameLength) else { alAcabar(); return }
        do { try conversor.convert(to: salida, from: entrada) } catch { alAcabar(); return }
        nodo.scheduleBuffer(salida, completionCallbackType: .dataPlayedBack) { _ in alAcabar() }
    }
}

final class Locutor: NSObject, AVSpeechSynthesizerDelegate, @unchecked Sendable {
    let sintetizador = AVSpeechSynthesizer()
    let voz: AVSpeechSynthesisVoice
    let reproductor: Reproductor?
    var cola: [String] = []
    var hablando = false
    var entradaCerrada = false
    var buferesSonando = 0          // solo con reproductor
    var sintesisAcabada = true      // solo con reproductor

    init(voz: AVSpeechSynthesisVoice, reproductor: Reproductor?) {
        self.voz = voz
        self.reproductor = reproductor
        super.init()
        sintetizador.delegate = self
        // Calienta la voz para que el primer trozo no espere.
        let vacia = AVSpeechUtterance(string: " ")
        vacia.voice = voz
        vacia.volume = 0
        if reproductor == nil { sintetizador.speak(vacia) } else { sintetizador.write(vacia) { _ in } }
    }

    func encolar(_ texto: String) {
        cola.append(texto)
        if !hablando { siguiente() }
    }

    func siguiente() {
        // Demasiado esperando: fuera lo más viejo.
        var saltadas = 0
        while cola.count > 1 && contarPalabras(cola) > palabrasMaximas {
            saltadas += cola.removeFirst().split(separator: " ").count
        }
        if saltadas > 0 { aviso("  (voz atrasada: me salto \(saltadas) palabras)") }

        guard !cola.isEmpty else {
            hablando = false
            if entradaCerrada { marcarHablando(false); exit(0) }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                if !self.hablando { self.marcarHablando(false) }
            }
            return
        }

        let palabras = contarPalabras(cola)
        let frase = AVSpeechUtterance(string: cola.joined(separator: " "))
        cola.removeAll()
        frase.voice = voz
        let prisa = Float(max(0, palabras - palabrasSinPrisa)) * 0.008
        frase.rate = min(ritmoMaximo, ritmoBase + prisa)
        hablando = true
        marcarHablando(true)

        guard let reproductor = reproductor else {
            sintetizador.speak(frase)
            return
        }
        // Con salida elegida: sintetizar a búferes y reproducirlos nosotros.
        sintesisAcabada = false
        sintetizador.write(frase) { bufer in
            DispatchQueue.main.async {
                guard let pcm = bufer as? AVAudioPCMBuffer, pcm.frameLength > 0 else {
                    self.sintesisAcabada = true         // búfer vacío = fin de la frase
                    if self.buferesSonando == 0 { self.siguiente() }
                    return
                }
                self.buferesSonando += 1
                reproductor.encolar(pcm) {
                    DispatchQueue.main.async {
                        self.buferesSonando -= 1
                        if self.buferesSonando == 0 && self.sintesisAcabada { self.siguiente() }
                    }
                }
            }
        }
    }

    func marcarHablando(_ si: Bool) {
        guard !ficheroHablando.isEmpty else { return }
        if si {
            FileManager.default.createFile(atPath: ficheroHablando, contents: nil)
        } else {
            try? FileManager.default.removeItem(atPath: ficheroHablando)
        }
    }

    func speechSynthesizer(_ s: AVSpeechSynthesizer, didFinish u: AVSpeechUtterance) {
        guard reproductor == nil, u.volume > 0 else { return }
        siguiente()
    }
}

guard let voz = buscarVoz(nombreVoz) else {
    aviso("locutor: no encuentro la voz \"\(nombreVoz)\" (say -v '?')")
    exit(1)
}

var reproductor: Reproductor? = nil
if !nombreSalida.isEmpty {
    if let destino = dispositivos().first(where: { $0.esSalida && $0.nombre == nombreSalida }) {
        // Si ya es la predeterminada, el camino normal es más directo.
        if destino.id != predeterminado(kAudioHardwarePropertyDefaultOutputDevice) {
            reproductor = Reproductor(dispositivo: destino.id)
            if reproductor == nil { aviso("locutor: no puedo usar la salida \"\(nombreSalida)\"; uso la predeterminada") }
        }
    } else {
        aviso("locutor: no encuentro la salida \"\(nombreSalida)\"; uso la predeterminada")
    }
}
let locutor = Locutor(voz: voz, reproductor: reproductor)

DispatchQueue.global().async {
    while let linea = readLine() {
        let texto = linea.trimmingCharacters(in: .whitespaces)
        if texto.isEmpty { continue }
        DispatchQueue.main.async { locutor.encolar(texto) }
    }
    // Fin de la entrada: termina de decir lo que quede y sale.
    DispatchQueue.main.async {
        locutor.entradaCerrada = true
        if !locutor.hablando { exit(0) }
    }
}
RunLoop.main.run()
