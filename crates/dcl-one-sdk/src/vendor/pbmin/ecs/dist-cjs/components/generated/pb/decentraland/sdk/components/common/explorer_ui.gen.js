"use strict";
/* eslint-disable */
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExplorerUi = void 0;
const protobufPackageSarasa = "decentraland.sdk.components.common";
/**
 * Identifies which fullscreen explorer panel OpenExplorerUi targets.
 * EU_SETTINGS holds 0 so an unset `ui` field defaults to the least-intrusive panel.
 */
/**
 * @public
 */
var ExplorerUi;
(function (ExplorerUi) {
    ExplorerUi[ExplorerUi["EU_SETTINGS"] = 0] = "EU_SETTINGS";
    ExplorerUi[ExplorerUi["EU_MAP"] = 1] = "EU_MAP";
    ExplorerUi[ExplorerUi["EU_BACKPACK"] = 2] = "EU_BACKPACK";
    ExplorerUi[ExplorerUi["EU_CAMERA_REEL"] = 3] = "EU_CAMERA_REEL";
    ExplorerUi[ExplorerUi["EU_COMMUNITIES"] = 4] = "EU_COMMUNITIES";
    ExplorerUi[ExplorerUi["EU_PLACES"] = 5] = "EU_PLACES";
    ExplorerUi[ExplorerUi["EU_EVENTS"] = 6] = "EU_EVENTS";
})(ExplorerUi = exports.ExplorerUi || (exports.ExplorerUi = {}));
