import { ParticleSystem, PBParticleSystem_BlendMode, PBParticleSystem_PlaybackState } from '../generated/index.gen';
export { PBParticleSystem_BlendMode as ParticleSystemBlendMode, PBParticleSystem_PlaybackState as ParticleSystemPlaybackState };
const ParticleSystemShapeHelper = {
    Point(point = {}) {
        return { $case: 'point', point };
    },
    Sphere(sphere = {}) {
        return { $case: 'sphere', sphere };
    },
    Cone(cone = {}) {
        return { $case: 'cone', cone };
    },
    Box(box = {}) {
        return { $case: 'box', box };
    }
};
export function defineParticleSystemComponent(engine) {
    const theComponent = ParticleSystem(engine);
    return {
        ...theComponent,
        Shape: ParticleSystemShapeHelper
    };
}
