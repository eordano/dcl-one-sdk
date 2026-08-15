import { LightSource } from '../generated/index.gen';
const LightSourceHelper = {
    Point(point) {
        return {
            $case: 'point',
            point
        };
    },
    Spot(spot) {
        return {
            $case: 'spot',
            spot
        };
    }
};
export function defineLightSourceComponent(engine) {
    const theComponent = LightSource(engine);
    return {
        ...theComponent,
        Type: LightSourceHelper
    };
}
