import type { IEngine } from '../engine';
import type { CompositeProvider } from './instance';
/**
 * Register the composite provider used to resolve composite files. Also registers
 * any `provider.schemas` on the engine pre-seal so composites that reference those
 * components can be instanced without further setup.
 * @public
 */
export declare function setCompositeProvider(engine: IEngine, provider: CompositeProvider): void;
/**
 * Get the composite provider registered via setCompositeProvider. Returns null if
 * no provider has been set.
 * @public
 */
export declare function getCompositeProvider(): CompositeProvider | null;
