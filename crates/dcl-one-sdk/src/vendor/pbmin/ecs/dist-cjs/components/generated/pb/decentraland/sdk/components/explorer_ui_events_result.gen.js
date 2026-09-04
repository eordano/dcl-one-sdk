"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.PBExplorerUiEventsResult_UiClosed = exports.PBExplorerUiEventsResult_UiOpened = exports.PBExplorerUiEventsResult = void 0;
/* eslint-disable */
const minimal_1 = __importDefault(require("protobufjs/minimal"));
const protobufPackageSarasa = "decentraland.sdk.components";
function createBasePBExplorerUiEventsResult() {
    return { ui: 0, timestamp: 0, event: undefined };
}
/**
 * @public
 */
var PBExplorerUiEventsResult;
(function (PBExplorerUiEventsResult) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.ui !== 0) {
            writer.uint32(8).int32(message.ui);
        }
        if (message.timestamp !== 0) {
            writer.uint32(16).uint32(message.timestamp);
        }
        switch (message.event?.$case) {
            case "opened":
                PBExplorerUiEventsResult_UiOpened.encode(message.event.opened, writer.uint32(82).fork()).ldelim();
                break;
            case "closed":
                PBExplorerUiEventsResult_UiClosed.encode(message.event.closed, writer.uint32(90).fork()).ldelim();
                break;
        }
        return writer;
    }
    PBExplorerUiEventsResult.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBExplorerUiEventsResult();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.ui = reader.int32();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.timestamp = reader.uint32();
                    continue;
                case 10:
                    if (tag !== 82) {
                        break;
                    }
                    message.event = {
                        $case: "opened",
                        opened: PBExplorerUiEventsResult_UiOpened.decode(reader, reader.uint32()),
                    };
                    continue;
                case 11:
                    if (tag !== 90) {
                        break;
                    }
                    message.event = {
                        $case: "closed",
                        closed: PBExplorerUiEventsResult_UiClosed.decode(reader, reader.uint32()),
                    };
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBExplorerUiEventsResult.decode = decode;
})(PBExplorerUiEventsResult = exports.PBExplorerUiEventsResult || (exports.PBExplorerUiEventsResult = {}));
function createBasePBExplorerUiEventsResult_UiOpened() {
    return {};
}
/**
 * @public
 */
var PBExplorerUiEventsResult_UiOpened;
(function (PBExplorerUiEventsResult_UiOpened) {
    function encode(_, writer = minimal_1.default.Writer.create()) {
        return writer;
    }
    PBExplorerUiEventsResult_UiOpened.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBExplorerUiEventsResult_UiOpened();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBExplorerUiEventsResult_UiOpened.decode = decode;
})(PBExplorerUiEventsResult_UiOpened = exports.PBExplorerUiEventsResult_UiOpened || (exports.PBExplorerUiEventsResult_UiOpened = {}));
function createBasePBExplorerUiEventsResult_UiClosed() {
    return {};
}
/**
 * @public
 */
var PBExplorerUiEventsResult_UiClosed;
(function (PBExplorerUiEventsResult_UiClosed) {
    function encode(_, writer = minimal_1.default.Writer.create()) {
        return writer;
    }
    PBExplorerUiEventsResult_UiClosed.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBExplorerUiEventsResult_UiClosed();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBExplorerUiEventsResult_UiClosed.decode = decode;
})(PBExplorerUiEventsResult_UiClosed = exports.PBExplorerUiEventsResult_UiClosed || (exports.PBExplorerUiEventsResult_UiClosed = {}));
