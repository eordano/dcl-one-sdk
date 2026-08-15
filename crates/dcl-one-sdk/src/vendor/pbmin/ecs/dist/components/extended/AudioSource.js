import { AudioSource } from '../generated/index.gen';
export function defineAudioSourceComponent(engine) {
    const theComponent = AudioSource(engine);
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
