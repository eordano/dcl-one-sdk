import { IEngine, LastWriteWinElementSetComponentDefinition } from '../../engine';
import { PBLightSource_Point, PBLightSource_Spot, PBLightSource } from '../generated/index.gen';
/**
 * @public
 */
export interface LightSourceHelper {
    /**
     * @returns a Light Source type
     */
    Point: (point: PBLightSource_Point) => PBLightSource['type'];
    /**
     * @returns a Light Source type
     */
    Spot: (spot: PBLightSource_Spot) => PBLightSource['type'];
}
/**
 * @public
 */
export interface LightSourceComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<PBLightSource> {
    /**
     * LightSource helper with constructor
     */
    Type: LightSourceHelper;
}
export declare function defineLightSourceComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): LightSourceComponentDefinitionExtended;
