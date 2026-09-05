import { TouchScreenControls } from '../generated/index.gen';
const ROOT_ENTITY = 0;
// Every on-screen gamepad action button.
const ALL_BUTTONS = [
    0 /* InputAction.IA_POINTER */,
    1 /* InputAction.IA_PRIMARY */,
    2 /* InputAction.IA_SECONDARY */,
    8 /* InputAction.IA_JUMP */,
    10 /* InputAction.IA_ACTION_3 */,
    11 /* InputAction.IA_ACTION_4 */,
    12 /* InputAction.IA_ACTION_5 */,
    13 /* InputAction.IA_ACTION_6 */
];
export function defineTouchScreenControlsComponent(engine) {
    const theComponent = TouchScreenControls(engine);
    // A mutable copy of the current RootEntity value (or a fresh default).
    function current() {
        const value = theComponent.getOrNull(ROOT_ENTITY);
        return {
            touchInputs: value?.touchInputs
                ? value.touchInputs.map((t) => ({ ...t, icon: t.icon ? { ...t.icon } : t.icon }))
                : [],
            mainAction: value?.mainAction,
            hideJoystick: value?.hideJoystick ?? false,
            hideCrosshair: value?.hideCrosshair ?? false
        };
    }
    function setHidden(actions) {
        const value = current();
        const byAction = new Map();
        for (const input of value.touchInputs)
            byAction.set(input.inputAction, input);
        for (const action of actions)
            byAction.set(action, { ...byAction.get(action), inputAction: action, hide: true });
        value.touchInputs = [...byAction.values()];
        theComponent.createOrReplace(ROOT_ENTITY, value);
    }
    return {
        ...theComponent,
        hideAll() {
            setHidden(ALL_BUTTONS);
        },
        showAll() {
            const value = current();
            value.touchInputs = [];
            theComponent.createOrReplace(ROOT_ENTITY, value);
        },
        hide(actions) {
            setHidden(actions);
        },
        setMainAction(action) {
            const value = current();
            value.mainAction = action;
            theComponent.createOrReplace(ROOT_ENTITY, value);
        },
        hideJoystick() {
            const value = current();
            value.hideJoystick = true;
            theComponent.createOrReplace(ROOT_ENTITY, value);
        },
        showJoystick() {
            const value = current();
            value.hideJoystick = false;
            theComponent.createOrReplace(ROOT_ENTITY, value);
        },
        hideCrosshair() {
            const value = current();
            value.hideCrosshair = true;
            theComponent.createOrReplace(ROOT_ENTITY, value);
        },
        showCrosshair() {
            const value = current();
            value.hideCrosshair = false;
            theComponent.createOrReplace(ROOT_ENTITY, value);
        }
    };
}
