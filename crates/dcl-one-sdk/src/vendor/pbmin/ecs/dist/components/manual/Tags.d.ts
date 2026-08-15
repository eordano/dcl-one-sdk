import { IEngine, LastWriteWinElementSetComponentDefinition } from '../../engine/types';
import { Entity } from '../../engine';
export interface TagsType {
    tags: string[];
}
export interface TagsComponentDefinitionExtended extends LastWriteWinElementSetComponentDefinition<TagsType> {
    /**
     * @public
     *
     * Add a tag to the entity's Tags component or create the component if it doesn't exist and add the tag
     * @param entity - entity to add the tag to
     * @param tagName - the tag name to add
     * @returns true
     */
    add(entity: Entity, tagName: string): boolean;
    /**
     * @public
     *
     * Remove a tag from the entity's Tags component
     * @param entity - entity to remove the tag from
     * @param tagName - the tag name to remove
     * @returns true if successful, false if the entity doesn't have a Tags component or the tag doesn't exist
     */
    remove(entity: Entity, tagName: string): boolean;
}
/**
 * @public
 *
 * Define the Tags component
 * @param engine - the engine to define the component on
 * @returns the Tags component definition
 */
declare function defineTagsComponent(engine: Pick<IEngine, 'defineComponent'>): TagsComponentDefinitionExtended;
export default defineTagsComponent;
