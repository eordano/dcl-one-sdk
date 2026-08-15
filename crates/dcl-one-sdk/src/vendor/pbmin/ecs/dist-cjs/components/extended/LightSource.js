"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.defineLightSourceComponent = void 0;
const index_gen_1 = require("../generated/index.gen");
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
function defineLightSourceComponent(engine) {
    const theComponent = (0, index_gen_1.LightSource)(engine);
    return {
        ...theComponent,
        Type: LightSourceHelper
    };
}
exports.defineLightSourceComponent = defineLightSourceComponent;
