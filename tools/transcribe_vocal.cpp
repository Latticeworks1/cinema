// Adapter around the locally built NeuralNote MuScriptor engine that restricts transcription to
// InstrumentGroup::Voice, instead of the unconditional (all-instrument) path transcribe_neuralnote.cpp
// uses. A non-empty `instruments` selection adds the instrument_group conditioning prefix and masks
// every other instrument's tokens, forcing the model to decode the vocal line instead of letting it
// compete with everything else in the mix.
#include "muscriptor/muscriptor.hpp"
#include <fstream>
#include <iostream>
#include <vector>
#include <iomanip>
int main(int argc, char** argv) {
    if (argc != 4) { std::cerr << "usage: transcribe_vocal MODEL INPUT.f32 OUTPUT.json\n"; return 2; }
    std::ifstream input(argv[2], std::ios::binary | std::ios::ate);
    if (!input) return 2;
    const auto bytes = input.tellg();
    if (bytes <= 0 || bytes % 4 != 0) return 2;
    std::vector<float> samples(static_cast<size_t>(bytes)/4);
    input.seekg(0); input.read(reinterpret_cast<char*>(samples.data()), bytes);
    msl::LoadOptions load;
    load.on_progress = [](float p) { if(p==1) std::cerr << "Weights loaded\n"; };
    std::cerr << "Loading model\n";
    auto engine = msl::Transcriber::load(argv[1],load);
    if (!engine) { std::cerr << msl::describe(engine.error()) << '\n'; return 1; }
    std::cerr << "Device: " << engine->device().name << " / " << engine->device().backend << '\n';
    msl::TranscribeOptions opts;
    opts.instruments = {msl::InstrumentGroup::Voice};
    auto notes = engine->transcribe(samples, opts, [](const msl::TranscriptionUpdate& u) {
        std::cerr << "Progress " << int(u.progress*100) << "%; finalized through " << u.finalized_through << "s; " << u.new_notes.size() << " notes\n";
        return true;
    });
    if (!notes) { std::cerr << msl::describe(notes.error()) << '\n'; return 1; }
    std::ofstream out(argv[3]);
    out << std::setprecision(10) << "{\"sample_rate\":16000,\"seconds\":" << samples.size()/16000.0 << ",\"notes\":[";
    bool first=true;
    for(const auto& n:*notes) {
        if(!first) out << ','; first=false;
        out << "{\"onset\":" << n.onset << ",\"offset\":" << n.offset << ",\"pitch\":" << n.pitch
            << ",\"program\":" << n.program << ",\"is_drum\":" << (n.is_drum?"true":"false")
            << ",\"instrument\":\"" << msl::instrumentLabel(n.program) << "\"}";
    }
    out << "]}\n";
    if (!out) return 1;
    std::cerr << "Saved " << notes->size() << " notes\n";
}
