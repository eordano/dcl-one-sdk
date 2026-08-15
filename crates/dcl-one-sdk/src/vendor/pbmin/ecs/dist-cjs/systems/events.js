"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || function (mod) {
    if (mod && mod.__esModule) return mod;
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
    __setModuleDefault(result, mod);
    return result;
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.createPointerEventsSystem = exports.getDefaultOpts = void 0;
const components = __importStar(require("../components"));
const entity_1 = require("../engine/entity");
const invariant_1 = require("../runtime/invariant");
const getDefaultOpts = (opts = {}) => ({
    button: 3 /* InputAction.IA_ANY */,
    ...opts
});
exports.getDefaultOpts = getDefaultOpts;
/**
 * @public
 * ___DO NOT USE___ use pointerEventsSystem instead
 */
function createPointerEventsSystem(engine, inputSystem) {
    const PointerEvents = components.PointerEvents(engine);
    let EventType;
    (function (EventType) {
        EventType[EventType["Click"] = 0] = "Click";
        EventType[EventType["Down"] = 1] = "Down";
        EventType[EventType["Up"] = 2] = "Up";
        EventType[EventType["HoverEnter"] = 3] = "HoverEnter";
        EventType[EventType["HoverLeave"] = 4] = "HoverLeave";
        EventType[EventType["ProximityEnter"] = 5] = "ProximityEnter";
        EventType[EventType["ProximityLeave"] = 6] = "ProximityLeave";
    })(EventType || (EventType = {}));
    const eventsMap = new Map();
    function getEvent(entity) {
        return eventsMap.get(entity) || eventsMap.set(entity, new Map()).get(entity);
    }
    function setPointerEvent(entity, type, opts, interactionType = 0 /* InteractionType.CURSOR */) {
        const pointerEvent = PointerEvents.getMutableOrNull(entity) || PointerEvents.create(entity);
        pointerEvent.pointerEvents.push({
            eventType: type,
            eventInfo: {
                button: opts.button,
                showFeedback: opts.showFeedback,
                showHighlight: opts.showHighlight,
                hoverText: opts.hoverText,
                maxDistance: opts.maxDistance,
                maxPlayerDistance: opts.maxPlayerDistance,
                priority: opts.priority
            },
            interactionType: interactionType ?? 0 /* InteractionType.CURSOR */
        });
    }
    function removePointerEvent(entity, type, button, interactionType = 0 /* InteractionType.CURSOR */) {
        const pointerEvent = PointerEvents.getMutableOrNull(entity);
        if (!pointerEvent)
            return;
        pointerEvent.pointerEvents = pointerEvent.pointerEvents.filter((pointer) => !(pointer.eventInfo?.button === button &&
            pointer.eventType === type &&
            pointer.interactionType === interactionType));
    }
    function getPointerEvent(eventType) {
        if (eventType === EventType.Up) {
            return 0 /* PointerEventType.PET_UP */;
        }
        else if (eventType === EventType.HoverLeave) {
            return 3 /* PointerEventType.PET_HOVER_LEAVE */;
        }
        else if (eventType === EventType.HoverEnter) {
            return 2 /* PointerEventType.PET_HOVER_ENTER */;
        }
        else if (eventType === EventType.ProximityEnter) {
            return 4 /* PointerEventType.PET_PROXIMITY_ENTER */;
        }
        else if (eventType === EventType.ProximityLeave) {
            return 5 /* PointerEventType.PET_PROXIMITY_LEAVE */;
        }
        return 1 /* PointerEventType.PET_DOWN */;
    }
    function removeEvent(entity, type, interactionType = 0 /* InteractionType.CURSOR */) {
        const event = getEvent(entity);
        const pointerEvent = event.get(type);
        if (pointerEvent?.opts.hoverText) {
            removePointerEvent(entity, getPointerEvent(type), pointerEvent.opts.button);
        }
        event.delete(type);
    }
    engine.addSystem(function EventSystem() {
        for (const [entity, event] of eventsMap) {
            if (engine.getEntityState(entity) === entity_1.EntityState.Removed) {
                eventsMap.delete(entity);
                continue;
            }
            for (const [eventType, { cb, opts }] of event) {
                if (eventType === EventType.Click) {
                    const command = inputSystem.getClick(opts.button, entity);
                    if (command)
                        (0, invariant_1.checkNotThenable)(cb(command.up), 'Click event returned a thenable. Only synchronous functions are allowed');
                }
                if (eventType === EventType.Down ||
                    eventType === EventType.Up ||
                    eventType === EventType.HoverEnter ||
                    eventType === EventType.HoverLeave ||
                    eventType === EventType.ProximityEnter ||
                    eventType === EventType.ProximityLeave) {
                    const command = inputSystem.getInputCommand(opts.button, getPointerEvent(eventType), entity);
                    if (command) {
                        (0, invariant_1.checkNotThenable)(cb(command), 'Event handler returned a thenable. Only synchronous functions are allowed');
                    }
                }
            }
        }
    });
    const onPointerDown = (...args) => {
        const [data, cb, maybeOpts] = args;
        if (typeof data === 'number') {
            return onPointerDown({ entity: data, opts: maybeOpts ?? {} }, cb);
        }
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.Down);
        getEvent(entity).set(EventType.Down, { cb, opts: options });
        setPointerEvent(entity, 1 /* PointerEventType.PET_DOWN */, options);
    };
    const onPointerUp = (...args) => {
        const [data, cb, maybeOpts] = args;
        if (typeof data === 'number') {
            return onPointerUp({ entity: data, opts: maybeOpts ?? {} }, cb);
        }
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.Up);
        getEvent(entity).set(EventType.Up, { cb, opts: options });
        setPointerEvent(entity, 0 /* PointerEventType.PET_UP */, options);
    };
    const onPointerHoverEnter = (...args) => {
        const [data, cb] = args;
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.HoverEnter);
        getEvent(entity).set(EventType.HoverEnter, { cb, opts: options });
        setPointerEvent(entity, 2 /* PointerEventType.PET_HOVER_ENTER */, options);
    };
    const onPointerHoverLeave = (...args) => {
        const [data, cb] = args;
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.HoverLeave);
        getEvent(entity).set(EventType.HoverLeave, { cb, opts: options });
        setPointerEvent(entity, 3 /* PointerEventType.PET_HOVER_LEAVE */, options);
    };
    const onProximityDown = (...args) => {
        const [data, cb] = args;
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.Down, 1 /* InteractionType.PROXIMITY */);
        getEvent(entity).set(EventType.Down, { cb, opts: options });
        setPointerEvent(entity, 1 /* PointerEventType.PET_DOWN */, options, 1 /* InteractionType.PROXIMITY */);
    };
    const onProximityUp = (...args) => {
        const [data, cb] = args;
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.Up, 1 /* InteractionType.PROXIMITY */);
        getEvent(entity).set(EventType.Up, { cb, opts: options });
        setPointerEvent(entity, 0 /* PointerEventType.PET_UP */, options, 1 /* InteractionType.PROXIMITY */);
    };
    const onProximityEnter = (...args) => {
        const [data, cb] = args;
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.ProximityEnter, 1 /* InteractionType.PROXIMITY */);
        getEvent(entity).set(EventType.ProximityEnter, { cb, opts: options });
        setPointerEvent(entity, 4 /* PointerEventType.PET_PROXIMITY_ENTER */, options, 1 /* InteractionType.PROXIMITY */);
    };
    const onProximityLeave = (...args) => {
        const [data, cb] = args;
        const { entity, opts } = data;
        const options = (0, exports.getDefaultOpts)(opts);
        removeEvent(entity, EventType.ProximityLeave, 1 /* InteractionType.PROXIMITY */);
        getEvent(entity).set(EventType.ProximityLeave, { cb, opts: options });
        setPointerEvent(entity, 5 /* PointerEventType.PET_PROXIMITY_LEAVE */, options, 1 /* InteractionType.PROXIMITY */);
    };
    return {
        removeOnClick(entity) {
            removeEvent(entity, EventType.Click);
        },
        removeOnPointerDown(entity) {
            removeEvent(entity, EventType.Down);
        },
        removeOnPointerUp(entity) {
            removeEvent(entity, EventType.Up);
        },
        removeOnPointerHoverEnter(entity) {
            removeEvent(entity, EventType.HoverEnter);
        },
        removeOnPointerHoverLeave(entity) {
            removeEvent(entity, EventType.HoverLeave);
        },
        removeOnProximityDown(entity) {
            removeEvent(entity, EventType.Down, 1 /* InteractionType.PROXIMITY */);
        },
        removeOnProximityUp(entity) {
            removeEvent(entity, EventType.Up, 1 /* InteractionType.PROXIMITY */);
        },
        removeOnProximityEnter(entity) {
            removeEvent(entity, EventType.ProximityEnter, 1 /* InteractionType.PROXIMITY */);
        },
        removeOnProximityLeave(entity) {
            removeEvent(entity, EventType.ProximityLeave, 1 /* InteractionType.PROXIMITY */);
        },
        onClick(value, cb) {
            const { entity } = value;
            const options = (0, exports.getDefaultOpts)(value.opts);
            // Clear previous event with over feedback included
            removeEvent(entity, EventType.Click);
            // Set new event
            getEvent(entity).set(EventType.Click, { cb, opts: options });
            setPointerEvent(entity, 1 /* PointerEventType.PET_DOWN */, options);
        },
        onPointerDown,
        onPointerUp,
        onPointerHoverEnter,
        onPointerHoverLeave,
        onProximityDown,
        onProximityUp,
        onProximityEnter,
        onProximityLeave
    };
}
exports.createPointerEventsSystem = createPointerEventsSystem;
