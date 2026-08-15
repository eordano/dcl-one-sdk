import { Schemas } from '../../schemas';
/**
 * @public
 *
 * Define the Tags component
 * @param engine - the engine to define the component on
 * @returns the Tags component definition
 */
function defineTagsComponent(engine) {
    const Tags = engine.defineComponent('core-schema::Tags', {
        tags: Schemas.Array(Schemas.String)
    });
    return {
        ...Tags,
        add(entity, tagName) {
            const tagsComponent = Tags.getMutableOrNull(entity);
            if (tagsComponent) {
                tagsComponent.tags.push(tagName);
            }
            else {
                Tags.createOrReplace(entity, { tags: [tagName] });
            }
            return true;
        },
        remove(entity, tagName) {
            const tagsComponent = Tags.getMutableOrNull(entity);
            if (!tagsComponent || !tagsComponent.tags)
                return false;
            const newTags = tagsComponent.tags.filter((tag) => tag !== tagName);
            if (newTags.length === tagsComponent.tags.length)
                return false;
            tagsComponent.tags = newTags;
            return true;
        }
    };
}
export default defineTagsComponent;
