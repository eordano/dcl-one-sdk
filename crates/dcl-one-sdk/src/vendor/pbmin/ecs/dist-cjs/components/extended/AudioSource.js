"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.defineAudioSourceComponent = void 0;
const index_gen_1 = require("../generated/index.gen");
function defineAudioSourceComponent(engine) {
    const theComponent = (0, index_gen_1.AudioSource)(engine);
    return {
        ...theComponent,
        playSound(entity, src, resetCursor = true) {
            const existing = theComponent.getOrNull(entity);
            theComponent.createOrReplace(entity, {
                ...existing,
                audioClipUrl: src,
                playing: true,
                currentTime: resetCursor ? 0 : existing?.currentTime ?? 0
            });
            return true;
        },
        stopSound(entity, resetCursor = true) {
            // Get the mutable to modify
            const audioSource = theComponent.getMutableOrNull(entity);
            if (!audioSource)
                return false;
            audioSource.playing = false;
            audioSource.currentTime = resetCursor ? 0 : audioSource.currentTime;
            return true;
        }
    };
}
exports.defineAudioSourceComponent = defineAudioSourceComponent;
