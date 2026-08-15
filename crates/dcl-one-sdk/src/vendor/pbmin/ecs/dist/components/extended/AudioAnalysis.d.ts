import { Entity, IEngine } from '../../engine';
import { LastWriteWinElementSetComponentDefinition } from '../../engine/component';
import { PBAudioAnalysis, PBAudioAnalysisMode } from '../generated/pb/decentraland/sdk/components/audio_analysis.gen';
export interface AudioAnalysisComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<PBAudioAnalysis> {
    /**
     * Reads the component data of `entity` into the provided `out` view.
     *
     * @throws Error if the entity does not have an AudioAnalysis component.
     * @param entity - The entity whose AudioAnalysis data will be read.
     * @param out - An existing AudioAnalysisView to populate with the latest values.
     */
    readIntoView(entity: Entity, out: AudioAnalysisView): void;
    /**
     * Attempts to read the component data of `entity` into the provided `out` view.
     *
     * @returns `true` if the component exists and data was written into `out`,
     *          `false` if the entity does not have an AudioAnalysis component.
     * @param entity - The entity whose AudioAnalysis data will be read.
     * @param out - An existing AudioAnalysisView to populate.
     */
    tryReadIntoView(entity: Entity, out: AudioAnalysisView): boolean;
    /**
     * Creates an AudioAnalysis component for the given `entity`.
     *
     * If a component already exists on the entity, this call fails (does not replace).
     *
     * @param entity - The entity to attach the component to.
     * @param mode - Analysis mode. Defaults to `PBAudioAnalysisMode.MODE_LOGARITHMIC`.
     * @param amplitudeGain - Optional amplitude gain multiplier.
     * @param bandsGain - Optional gain multiplier applied to all frequency bands.
     */
    createAudioAnalysis(entity: Entity, mode?: PBAudioAnalysisMode, // default is PBAudioAnalysisMode.MODE_LOGARITHMIC
    amplitudeGain?: number, bandsGain?: number): void;
    /**
     * Creates the AudioAnalysis component if missing, or replaces the existing one.
     *
     * @param entity - The target entity.
     * @param mode - Analysis mode. Defaults to `PBAudioAnalysisMode.MODE_LOGARITHMIC`.
     * @param amplitudeGain - Optional amplitude gain multiplier.
     * @param bandsGain - Optional gain multiplier applied to the frequency bands.
     */
    createOrReplaceAudioAnalysis(entity: Entity, mode?: PBAudioAnalysisMode, // default is PBAudioAnalysisMode.MODE_LOGARITHMIC
    amplitudeGain?: number, bandsGain?: number): void;
}
/**
 * A read-only JavaScript-friendly view of AudioAnalysis ECS data.
 *
 * `amplitude` represents the aggregated signal strength.
 * `bands` represents the processed frequency bands.
 */
export type AudioAnalysisView = {
    amplitude: number;
    bands: number[];
};
export declare function defineAudioAnalysisComponent(engine: Pick<IEngine, 'defineComponentFromSchema'>): AudioAnalysisComponentDefinitionExtended;
