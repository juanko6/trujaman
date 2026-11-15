// Locutor: lee trozos de texto por la entrada estándar (uno por línea) y los dice en alto.
// Se queda abierto con la voz cargada, así cada trozo empieza a sonar enseguida
// (`say` tarda casi un segundo en arrancar cada vez).
//
//   locutor [VOZ] [FICHERO_HABLANDO]
//
// VOZ: nombre como sale en `say -v '?'` ("Samantha", "Daniel", "Ava (Premium)").
// FICHERO_HABLANDO: si se da (y no es ""), existe mientras habla y 0,4 s más, por el
// eco de la sala. El intérprete lo usa para no escucharse a sí mismo por los altavoces.
//
// Lleva su propia cola para no quedarse atrás cuando llega texto más rápido de lo
// que puede decirlo:
//   - al acabar un trozo, junta todo lo que esté esperando y lo dice de un tirón;
//   - cuanto más hay esperando, más rápido habla;
//   - si hay demasiado, se salta lo más viejo (como los subtítulos en directo).
//
// Compilar: swiftc -O locutor.swift -o locutor
import AVFoundation
import Foundation

let args = CommandLine.arguments
let nombreVoz = args.count > 1 ? args[1] : "Samantha"
let ficheroHablando = args.count > 2 ? args[2] : ""

let ritmoBase = AVSpeechUtteranceDefaultSpeechRate          // 0,5
let ritmoMaximo: Float = 0.62                               // más rápido deja de entenderse
let palabrasSinPrisa = 5                                    // hasta aquí, ritmo normal
let palabrasMaximas = 28                                    // más que esto esperando: se salta lo viejo

func buscarVoz(_ nombre: String) -> AVSpeechSynthesisVoice? {
    let voces = AVSpeechSynthesisVoice.speechVoices()
    let base = nombre.components(separatedBy: " (").first ?? nombre
    let premium = nombre.lowercased().contains("premium")
    let mejorada = nombre.lowercased().contains("mejorada") || nombre.lowercased().contains("enhanced")
    let candidatas = voces.filter { $0.name == base }
    if premium, let v = candidatas.first(where: { $0.quality == .premium }) { return v }
    if mejorada, let v = candidatas.first(where: { $0.quality == .enhanced }) { return v }
    // Sin calificativo: la mejor calidad instalada con ese nombre.
    return candidatas.max { $0.quality.rawValue < $1.quality.rawValue }
}

func aviso(_ texto: String) {
    FileHandle.standardError.write((texto + "\n").data(using: .utf8)!)
}

func contarPalabras(_ trozos: [String]) -> Int {
    trozos.reduce(0) { $0 + $1.split(separator: " ").count }
}

final class Locutor: NSObject, AVSpeechSynthesizerDelegate, @unchecked Sendable {
    let sintetizador = AVSpeechSynthesizer()
    let voz: AVSpeechSynthesisVoice
    var cola: [String] = []
    var hablando = false
    var entradaCerrada = false

    init(voz: AVSpeechSynthesisVoice) {
        self.voz = voz
        super.init()
        sintetizador.delegate = self
        // Calienta la voz para que el primer trozo no espere.
        let vacia = AVSpeechUtterance(string: " ")
        vacia.voice = voz
        vacia.volume = 0
        sintetizador.speak(vacia)
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
        sintetizador.speak(frase)
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
        guard u.volume > 0 else { return }
        siguiente()
    }
}

guard let voz = buscarVoz(nombreVoz) else {
    aviso("locutor: no encuentro la voz \"\(nombreVoz)\" (say -v '?')")
    exit(1)
}
let locutor = Locutor(voz: voz)

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
