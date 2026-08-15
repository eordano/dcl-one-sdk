/* eslint-disable */
const protobufPackageSarasa = "decentraland.sdk.components.common";
/**
 * @public
 */
export var InputAction;
(function (InputAction) {
    InputAction[InputAction["IA_POINTER"] = 0] = "IA_POINTER";
    InputAction[InputAction["IA_PRIMARY"] = 1] = "IA_PRIMARY";
    InputAction[InputAction["IA_SECONDARY"] = 2] = "IA_SECONDARY";
    InputAction[InputAction["IA_ANY"] = 3] = "IA_ANY";
    InputAction[InputAction["IA_FORWARD"] = 4] = "IA_FORWARD";
    InputAction[InputAction["IA_BACKWARD"] = 5] = "IA_BACKWARD";
    InputAction[InputAction["IA_RIGHT"] = 6] = "IA_RIGHT";
    InputAction[InputAction["IA_LEFT"] = 7] = "IA_LEFT";
    InputAction[InputAction["IA_JUMP"] = 8] = "IA_JUMP";
    InputAction[InputAction["IA_WALK"] = 9] = "IA_WALK";
    InputAction[InputAction["IA_ACTION_3"] = 10] = "IA_ACTION_3";
    InputAction[InputAction["IA_ACTION_4"] = 11] = "IA_ACTION_4";
    InputAction[InputAction["IA_ACTION_5"] = 12] = "IA_ACTION_5";
    InputAction[InputAction["IA_ACTION_6"] = 13] = "IA_ACTION_6";
    /** IA_MODIFIER - Modifier key (Shift on desktop) */
    InputAction[InputAction["IA_MODIFIER"] = 14] = "IA_MODIFIER";
})(InputAction || (InputAction = {}));
/** PointerEventType is a kind of interaction that can be detected. */
/**
 * @public
 */
export var PointerEventType;
(function (PointerEventType) {
    PointerEventType[PointerEventType["PET_UP"] = 0] = "PET_UP";
    PointerEventType[PointerEventType["PET_DOWN"] = 1] = "PET_DOWN";
    PointerEventType[PointerEventType["PET_HOVER_ENTER"] = 2] = "PET_HOVER_ENTER";
    PointerEventType[PointerEventType["PET_HOVER_LEAVE"] = 3] = "PET_HOVER_LEAVE";
    PointerEventType[PointerEventType["PET_PROXIMITY_ENTER"] = 4] = "PET_PROXIMITY_ENTER";
    PointerEventType[PointerEventType["PET_PROXIMITY_LEAVE"] = 5] = "PET_PROXIMITY_LEAVE";
})(PointerEventType || (PointerEventType = {}));
/**
 * @public
 */
export var InteractionType;
(function (InteractionType) {
    InteractionType[InteractionType["CURSOR"] = 0] = "CURSOR";
    InteractionType[InteractionType["PROXIMITY"] = 1] = "PROXIMITY";
})(InteractionType || (InteractionType = {}));
