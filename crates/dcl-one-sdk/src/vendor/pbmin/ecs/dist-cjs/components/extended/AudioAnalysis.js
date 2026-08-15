"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.defineAudioAnalysisComponent = void 0;
const index_gen_1 = require("../generated/index.gen");
function defineAudioAnalysisComponent(engine) {
    const theComponent = (0, index_gen_1.AudioAnalysis)(engine);
    return {
        ...theComponent,
        readIntoView(entity, out) {
            const audioAnalysis = theComponent.get(entity);
            out.amplitude = audioAnalysis.amplitude;
            out.bands[0] = audioAnalysis.band0;
            out.bands[1] = audioAnalysis.band1;
            out.bands[2] = audioAnalysis.band2;
            out.bands[3] = audioAnalysis.band3;
            out.bands[4] = audioAnalysis.band4;
            out.bands[5] = audioAnalysis.band5;
            out.bands[6] = audioAnalysis.band6;
            out.bands[7] = audioAnalysis.band7;
        },
        tryReadIntoView(entity, out) {
            const audioAnalysis = theComponent.getOrNull(entity);
            if (!audioAnalysis)
                return false;
            out.amplitude = audioAnalysis.amplitude;
            out.bands[0] = audioAnalysis.band0;
            out.bands[1] = audioAnalysis.band1;
            out.bands[2] = audioAnalysis.band2;
            out.bands[3] = audioAnalysis.band3;
            out.bands[4] = audioAnalysis.band4;
            out.bands[5] = audioAnalysis.band5;
            out.bands[6] = audioAnalysis.band6;
            out.bands[7] = audioAnalysis.band7;
            return true;
        },
        createAudioAnalysis(entity, mode, amplitudeGain, bandsGain) {
            theComponent.create(entity, {
                mode: mode || 1 /* PBAudioAnalysisMode.MODE_LOGARITHMIC */,
                amplitudeGain: amplitudeGain ?? undefined,
                bandsGain: bandsGain ?? undefined,
                amplitude: 0,
                band0: 0,
                band1: 0,
                band2: 0,
                band3: 0,
                band4: 0,
                band5: 0,
                band6: 0,
                band7: 0
            });
        },
        createOrReplaceAudioAnalysis(entity, mode, amplitudeGain, bandsGain) {
            theComponent.createOrReplace(entity, {
                mode: mode || 1 /* PBAudioAnalysisMode.MODE_LOGARITHMIC */,
                amplitudeGain: amplitudeGain ?? undefined,
                bandsGain: bandsGain ?? undefined,
                amplitude: 0,
                band0: 0,
                band1: 0,
                band2: 0,
                band3: 0,
                band4: 0,
                band5: 0,
                band6: 0,
                band7: 0
            });
        }
    };
}
exports.defineAudioAnalysisComponent = defineAudioAnalysisComponent;
