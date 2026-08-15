"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.DataServiceDefinition = exports.GetFilesListResponse_FileResult = exports.GetFilesListResponse = exports.GetFilesListRequest = exports.RemoveFilesResponse = exports.RemoveFilesRequest = exports.UndoRedoStateResponse = exports.RenameCustomAssetRequest = exports.DeleteCustomAssetRequest = exports.GetCustomAssetsResponse = exports.CreateCustomAssetResponse = exports.CreateCustomAssetRequest = exports.GetFileResponse = exports.GetFileRequest = exports.CopyFileRequest = exports.InspectorPreferencesMessage = exports.ImportAssetRequest_ContentEntry = exports.ImportAssetRequest = exports.AssetCatalogResponse = exports.Asset = exports.SaveFileRequest = exports.GetFilesSizesResponse_FileSize = exports.GetFilesSizesResponse = exports.GetFilesSizesRequest = exports.GetFilesResponse_File = exports.GetFilesResponse = exports.GetFilesRequest = exports.AssetData = exports.CrdtStreamMessage = exports.UndoRedoResponse = exports.Empty = exports.protobufPackage = void 0;
/* eslint-disable */
const long_1 = __importDefault(require("long"));
const minimal_1 = __importDefault(require("protobufjs/minimal"));
exports.protobufPackage = "";
function createBaseEmpty() {
    return {};
}
var Empty;
(function (Empty) {
    function encode(_, writer = minimal_1.default.Writer.create()) {
        return writer;
    }
    Empty.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseEmpty();
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
    Empty.decode = decode;
    function fromJSON(_) {
        return {};
    }
    Empty.fromJSON = fromJSON;
    function toJSON(_) {
        const obj = {};
        return obj;
    }
    Empty.toJSON = toJSON;
    function create(base) {
        return Empty.fromPartial(base ?? {});
    }
    Empty.create = create;
    function fromPartial(_) {
        const message = createBaseEmpty();
        return message;
    }
    Empty.fromPartial = fromPartial;
})(Empty || (exports.Empty = Empty = {}));
function createBaseUndoRedoResponse() {
    return { type: "" };
}
var UndoRedoResponse;
(function (UndoRedoResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.type !== "") {
            writer.uint32(10).string(message.type);
        }
        return writer;
    }
    UndoRedoResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseUndoRedoResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.type = reader.string();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    UndoRedoResponse.decode = decode;
    function fromJSON(object) {
        return { type: isSet(object.type) ? String(object.type) : "" };
    }
    UndoRedoResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.type !== undefined && (obj.type = message.type);
        return obj;
    }
    UndoRedoResponse.toJSON = toJSON;
    function create(base) {
        return UndoRedoResponse.fromPartial(base ?? {});
    }
    UndoRedoResponse.create = create;
    function fromPartial(object) {
        const message = createBaseUndoRedoResponse();
        message.type = object.type ?? "";
        return message;
    }
    UndoRedoResponse.fromPartial = fromPartial;
})(UndoRedoResponse || (exports.UndoRedoResponse = UndoRedoResponse = {}));
function createBaseCrdtStreamMessage() {
    return { data: new Uint8Array(0) };
}
var CrdtStreamMessage;
(function (CrdtStreamMessage) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.data.length !== 0) {
            writer.uint32(10).bytes(message.data);
        }
        return writer;
    }
    CrdtStreamMessage.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseCrdtStreamMessage();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.data = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    CrdtStreamMessage.decode = decode;
    function fromJSON(object) {
        return { data: isSet(object.data) ? bytesFromBase64(object.data) : new Uint8Array(0) };
    }
    CrdtStreamMessage.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.data !== undefined &&
            (obj.data = base64FromBytes(message.data !== undefined ? message.data : new Uint8Array(0)));
        return obj;
    }
    CrdtStreamMessage.toJSON = toJSON;
    function create(base) {
        return CrdtStreamMessage.fromPartial(base ?? {});
    }
    CrdtStreamMessage.create = create;
    function fromPartial(object) {
        const message = createBaseCrdtStreamMessage();
        message.data = object.data ?? new Uint8Array(0);
        return message;
    }
    CrdtStreamMessage.fromPartial = fromPartial;
})(CrdtStreamMessage || (exports.CrdtStreamMessage = CrdtStreamMessage = {}));
function createBaseAssetData() {
    return { data: new Uint8Array(0) };
}
var AssetData;
(function (AssetData) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.data.length !== 0) {
            writer.uint32(10).bytes(message.data);
        }
        return writer;
    }
    AssetData.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseAssetData();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.data = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    AssetData.decode = decode;
    function fromJSON(object) {
        return { data: isSet(object.data) ? bytesFromBase64(object.data) : new Uint8Array(0) };
    }
    AssetData.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.data !== undefined &&
            (obj.data = base64FromBytes(message.data !== undefined ? message.data : new Uint8Array(0)));
        return obj;
    }
    AssetData.toJSON = toJSON;
    function create(base) {
        return AssetData.fromPartial(base ?? {});
    }
    AssetData.create = create;
    function fromPartial(object) {
        const message = createBaseAssetData();
        message.data = object.data ?? new Uint8Array(0);
        return message;
    }
    AssetData.fromPartial = fromPartial;
})(AssetData || (exports.AssetData = AssetData = {}));
function createBaseGetFilesRequest() {
    return { path: "", ignore: [] };
}
var GetFilesRequest;
(function (GetFilesRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        for (const v of message.ignore) {
            writer.uint32(18).string(v);
        }
        return writer;
    }
    GetFilesRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.ignore.push(reader.string());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesRequest.decode = decode;
    function fromJSON(object) {
        return {
            path: isSet(object.path) ? String(object.path) : "",
            ignore: Array.isArray(object?.ignore) ? object.ignore.map((e) => String(e)) : [],
        };
    }
    GetFilesRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        if (message.ignore) {
            obj.ignore = message.ignore.map((e) => e);
        }
        else {
            obj.ignore = [];
        }
        return obj;
    }
    GetFilesRequest.toJSON = toJSON;
    function create(base) {
        return GetFilesRequest.fromPartial(base ?? {});
    }
    GetFilesRequest.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesRequest();
        message.path = object.path ?? "";
        message.ignore = object.ignore?.map((e) => e) || [];
        return message;
    }
    GetFilesRequest.fromPartial = fromPartial;
})(GetFilesRequest || (exports.GetFilesRequest = GetFilesRequest = {}));
function createBaseGetFilesResponse() {
    return { files: [] };
}
var GetFilesResponse;
(function (GetFilesResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.files) {
            GetFilesResponse_File.encode(v, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    GetFilesResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.files.push(GetFilesResponse_File.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesResponse.decode = decode;
    function fromJSON(object) {
        return {
            files: Array.isArray(object?.files) ? object.files.map((e) => GetFilesResponse_File.fromJSON(e)) : [],
        };
    }
    GetFilesResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.files) {
            obj.files = message.files.map((e) => e ? GetFilesResponse_File.toJSON(e) : undefined);
        }
        else {
            obj.files = [];
        }
        return obj;
    }
    GetFilesResponse.toJSON = toJSON;
    function create(base) {
        return GetFilesResponse.fromPartial(base ?? {});
    }
    GetFilesResponse.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesResponse();
        message.files = object.files?.map((e) => GetFilesResponse_File.fromPartial(e)) || [];
        return message;
    }
    GetFilesResponse.fromPartial = fromPartial;
})(GetFilesResponse || (exports.GetFilesResponse = GetFilesResponse = {}));
function createBaseGetFilesResponse_File() {
    return { path: "", content: new Uint8Array(0) };
}
var GetFilesResponse_File;
(function (GetFilesResponse_File) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        if (message.content.length !== 0) {
            writer.uint32(18).bytes(message.content);
        }
        return writer;
    }
    GetFilesResponse_File.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesResponse_File();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.content = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesResponse_File.decode = decode;
    function fromJSON(object) {
        return {
            path: isSet(object.path) ? String(object.path) : "",
            content: isSet(object.content) ? bytesFromBase64(object.content) : new Uint8Array(0),
        };
    }
    GetFilesResponse_File.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        message.content !== undefined &&
            (obj.content = base64FromBytes(message.content !== undefined ? message.content : new Uint8Array(0)));
        return obj;
    }
    GetFilesResponse_File.toJSON = toJSON;
    function create(base) {
        return GetFilesResponse_File.fromPartial(base ?? {});
    }
    GetFilesResponse_File.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesResponse_File();
        message.path = object.path ?? "";
        message.content = object.content ?? new Uint8Array(0);
        return message;
    }
    GetFilesResponse_File.fromPartial = fromPartial;
})(GetFilesResponse_File || (exports.GetFilesResponse_File = GetFilesResponse_File = {}));
function createBaseGetFilesSizesRequest() {
    return { path: "", ignore: [] };
}
var GetFilesSizesRequest;
(function (GetFilesSizesRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        for (const v of message.ignore) {
            writer.uint32(18).string(v);
        }
        return writer;
    }
    GetFilesSizesRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesSizesRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.ignore.push(reader.string());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesSizesRequest.decode = decode;
    function fromJSON(object) {
        return {
            path: isSet(object.path) ? String(object.path) : "",
            ignore: Array.isArray(object?.ignore) ? object.ignore.map((e) => String(e)) : [],
        };
    }
    GetFilesSizesRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        if (message.ignore) {
            obj.ignore = message.ignore.map((e) => e);
        }
        else {
            obj.ignore = [];
        }
        return obj;
    }
    GetFilesSizesRequest.toJSON = toJSON;
    function create(base) {
        return GetFilesSizesRequest.fromPartial(base ?? {});
    }
    GetFilesSizesRequest.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesSizesRequest();
        message.path = object.path ?? "";
        message.ignore = object.ignore?.map((e) => e) || [];
        return message;
    }
    GetFilesSizesRequest.fromPartial = fromPartial;
})(GetFilesSizesRequest || (exports.GetFilesSizesRequest = GetFilesSizesRequest = {}));
function createBaseGetFilesSizesResponse() {
    return { files: [] };
}
var GetFilesSizesResponse;
(function (GetFilesSizesResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.files) {
            GetFilesSizesResponse_FileSize.encode(v, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    GetFilesSizesResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesSizesResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.files.push(GetFilesSizesResponse_FileSize.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesSizesResponse.decode = decode;
    function fromJSON(object) {
        return {
            files: Array.isArray(object?.files)
                ? object.files.map((e) => GetFilesSizesResponse_FileSize.fromJSON(e))
                : [],
        };
    }
    GetFilesSizesResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.files) {
            obj.files = message.files.map((e) => e ? GetFilesSizesResponse_FileSize.toJSON(e) : undefined);
        }
        else {
            obj.files = [];
        }
        return obj;
    }
    GetFilesSizesResponse.toJSON = toJSON;
    function create(base) {
        return GetFilesSizesResponse.fromPartial(base ?? {});
    }
    GetFilesSizesResponse.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesSizesResponse();
        message.files = object.files?.map((e) => GetFilesSizesResponse_FileSize.fromPartial(e)) || [];
        return message;
    }
    GetFilesSizesResponse.fromPartial = fromPartial;
})(GetFilesSizesResponse || (exports.GetFilesSizesResponse = GetFilesSizesResponse = {}));
function createBaseGetFilesSizesResponse_FileSize() {
    return { path: "", size: 0 };
}
var GetFilesSizesResponse_FileSize;
(function (GetFilesSizesResponse_FileSize) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        if (message.size !== 0) {
            writer.uint32(16).int64(message.size);
        }
        return writer;
    }
    GetFilesSizesResponse_FileSize.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesSizesResponse_FileSize();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.size = longToNumber(reader.int64());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesSizesResponse_FileSize.decode = decode;
    function fromJSON(object) {
        return { path: isSet(object.path) ? String(object.path) : "", size: isSet(object.size) ? Number(object.size) : 0 };
    }
    GetFilesSizesResponse_FileSize.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        message.size !== undefined && (obj.size = Math.round(message.size));
        return obj;
    }
    GetFilesSizesResponse_FileSize.toJSON = toJSON;
    function create(base) {
        return GetFilesSizesResponse_FileSize.fromPartial(base ?? {});
    }
    GetFilesSizesResponse_FileSize.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesSizesResponse_FileSize();
        message.path = object.path ?? "";
        message.size = object.size ?? 0;
        return message;
    }
    GetFilesSizesResponse_FileSize.fromPartial = fromPartial;
})(GetFilesSizesResponse_FileSize || (exports.GetFilesSizesResponse_FileSize = GetFilesSizesResponse_FileSize = {}));
function createBaseSaveFileRequest() {
    return { path: "", content: new Uint8Array(0) };
}
var SaveFileRequest;
(function (SaveFileRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        if (message.content.length !== 0) {
            writer.uint32(18).bytes(message.content);
        }
        return writer;
    }
    SaveFileRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseSaveFileRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.content = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    SaveFileRequest.decode = decode;
    function fromJSON(object) {
        return {
            path: isSet(object.path) ? String(object.path) : "",
            content: isSet(object.content) ? bytesFromBase64(object.content) : new Uint8Array(0),
        };
    }
    SaveFileRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        message.content !== undefined &&
            (obj.content = base64FromBytes(message.content !== undefined ? message.content : new Uint8Array(0)));
        return obj;
    }
    SaveFileRequest.toJSON = toJSON;
    function create(base) {
        return SaveFileRequest.fromPartial(base ?? {});
    }
    SaveFileRequest.create = create;
    function fromPartial(object) {
        const message = createBaseSaveFileRequest();
        message.path = object.path ?? "";
        message.content = object.content ?? new Uint8Array(0);
        return message;
    }
    SaveFileRequest.fromPartial = fromPartial;
})(SaveFileRequest || (exports.SaveFileRequest = SaveFileRequest = {}));
function createBaseAsset() {
    return { path: "", skipUndo: undefined };
}
var Asset;
(function (Asset) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        if (message.skipUndo !== undefined) {
            writer.uint32(16).bool(message.skipUndo);
        }
        return writer;
    }
    Asset.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseAsset();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.skipUndo = reader.bool();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    Asset.decode = decode;
    function fromJSON(object) {
        return {
            path: isSet(object.path) ? String(object.path) : "",
            skipUndo: isSet(object.skipUndo) ? Boolean(object.skipUndo) : undefined,
        };
    }
    Asset.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        message.skipUndo !== undefined && (obj.skipUndo = message.skipUndo);
        return obj;
    }
    Asset.toJSON = toJSON;
    function create(base) {
        return Asset.fromPartial(base ?? {});
    }
    Asset.create = create;
    function fromPartial(object) {
        const message = createBaseAsset();
        message.path = object.path ?? "";
        message.skipUndo = object.skipUndo ?? undefined;
        return message;
    }
    Asset.fromPartial = fromPartial;
})(Asset || (exports.Asset = Asset = {}));
function createBaseAssetCatalogResponse() {
    return { basePath: "", assets: [] };
}
var AssetCatalogResponse;
(function (AssetCatalogResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.basePath !== "") {
            writer.uint32(10).string(message.basePath);
        }
        for (const v of message.assets) {
            Asset.encode(v, writer.uint32(18).fork()).ldelim();
        }
        return writer;
    }
    AssetCatalogResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseAssetCatalogResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.basePath = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.assets.push(Asset.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    AssetCatalogResponse.decode = decode;
    function fromJSON(object) {
        return {
            basePath: isSet(object.basePath) ? String(object.basePath) : "",
            assets: Array.isArray(object?.assets) ? object.assets.map((e) => Asset.fromJSON(e)) : [],
        };
    }
    AssetCatalogResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.basePath !== undefined && (obj.basePath = message.basePath);
        if (message.assets) {
            obj.assets = message.assets.map((e) => e ? Asset.toJSON(e) : undefined);
        }
        else {
            obj.assets = [];
        }
        return obj;
    }
    AssetCatalogResponse.toJSON = toJSON;
    function create(base) {
        return AssetCatalogResponse.fromPartial(base ?? {});
    }
    AssetCatalogResponse.create = create;
    function fromPartial(object) {
        const message = createBaseAssetCatalogResponse();
        message.basePath = object.basePath ?? "";
        message.assets = object.assets?.map((e) => Asset.fromPartial(e)) || [];
        return message;
    }
    AssetCatalogResponse.fromPartial = fromPartial;
})(AssetCatalogResponse || (exports.AssetCatalogResponse = AssetCatalogResponse = {}));
function createBaseImportAssetRequest() {
    return { basePath: "", assetPackageName: "", content: new Map() };
}
var ImportAssetRequest;
(function (ImportAssetRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.basePath !== "") {
            writer.uint32(10).string(message.basePath);
        }
        if (message.assetPackageName !== "") {
            writer.uint32(18).string(message.assetPackageName);
        }
        message.content.forEach((value, key) => {
            ImportAssetRequest_ContentEntry.encode({ key: key, value }, writer.uint32(26).fork()).ldelim();
        });
        return writer;
    }
    ImportAssetRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseImportAssetRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.basePath = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.assetPackageName = reader.string();
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    const entry3 = ImportAssetRequest_ContentEntry.decode(reader, reader.uint32());
                    if (entry3.value !== undefined) {
                        message.content.set(entry3.key, entry3.value);
                    }
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    ImportAssetRequest.decode = decode;
    function fromJSON(object) {
        return {
            basePath: isSet(object.basePath) ? String(object.basePath) : "",
            assetPackageName: isSet(object.assetPackageName) ? String(object.assetPackageName) : "",
            content: isObject(object.content)
                ? Object.entries(object.content).reduce((acc, [key, value]) => {
                    acc.set(key, bytesFromBase64(value));
                    return acc;
                }, new Map())
                : new Map(),
        };
    }
    ImportAssetRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.basePath !== undefined && (obj.basePath = message.basePath);
        message.assetPackageName !== undefined && (obj.assetPackageName = message.assetPackageName);
        obj.content = {};
        if (message.content) {
            message.content.forEach((v, k) => {
                obj.content[k] = base64FromBytes(v);
            });
        }
        return obj;
    }
    ImportAssetRequest.toJSON = toJSON;
    function create(base) {
        return ImportAssetRequest.fromPartial(base ?? {});
    }
    ImportAssetRequest.create = create;
    function fromPartial(object) {
        const message = createBaseImportAssetRequest();
        message.basePath = object.basePath ?? "";
        message.assetPackageName = object.assetPackageName ?? "";
        message.content = (() => {
            const m = new Map();
            (object.content ?? new Map()).forEach((value, key) => {
                if (value !== undefined) {
                    m.set(key, value);
                }
            });
            return m;
        })();
        return message;
    }
    ImportAssetRequest.fromPartial = fromPartial;
})(ImportAssetRequest || (exports.ImportAssetRequest = ImportAssetRequest = {}));
function createBaseImportAssetRequest_ContentEntry() {
    return { key: "", value: new Uint8Array(0) };
}
var ImportAssetRequest_ContentEntry;
(function (ImportAssetRequest_ContentEntry) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.key !== "") {
            writer.uint32(10).string(message.key);
        }
        if (message.value.length !== 0) {
            writer.uint32(18).bytes(message.value);
        }
        return writer;
    }
    ImportAssetRequest_ContentEntry.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseImportAssetRequest_ContentEntry();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.key = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.value = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    ImportAssetRequest_ContentEntry.decode = decode;
    function fromJSON(object) {
        return {
            key: isSet(object.key) ? String(object.key) : "",
            value: isSet(object.value) ? bytesFromBase64(object.value) : new Uint8Array(0),
        };
    }
    ImportAssetRequest_ContentEntry.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.key !== undefined && (obj.key = message.key);
        message.value !== undefined &&
            (obj.value = base64FromBytes(message.value !== undefined ? message.value : new Uint8Array(0)));
        return obj;
    }
    ImportAssetRequest_ContentEntry.toJSON = toJSON;
    function create(base) {
        return ImportAssetRequest_ContentEntry.fromPartial(base ?? {});
    }
    ImportAssetRequest_ContentEntry.create = create;
    function fromPartial(object) {
        const message = createBaseImportAssetRequest_ContentEntry();
        message.key = object.key ?? "";
        message.value = object.value ?? new Uint8Array(0);
        return message;
    }
    ImportAssetRequest_ContentEntry.fromPartial = fromPartial;
})(ImportAssetRequest_ContentEntry || (exports.ImportAssetRequest_ContentEntry = ImportAssetRequest_ContentEntry = {}));
function createBaseInspectorPreferencesMessage() {
    return { freeCameraInvertRotation: false, autosaveEnabled: false };
}
var InspectorPreferencesMessage;
(function (InspectorPreferencesMessage) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.freeCameraInvertRotation === true) {
            writer.uint32(8).bool(message.freeCameraInvertRotation);
        }
        if (message.autosaveEnabled === true) {
            writer.uint32(16).bool(message.autosaveEnabled);
        }
        return writer;
    }
    InspectorPreferencesMessage.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseInspectorPreferencesMessage();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.freeCameraInvertRotation = reader.bool();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.autosaveEnabled = reader.bool();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    InspectorPreferencesMessage.decode = decode;
    function fromJSON(object) {
        return {
            freeCameraInvertRotation: isSet(object.freeCameraInvertRotation)
                ? Boolean(object.freeCameraInvertRotation)
                : false,
            autosaveEnabled: isSet(object.autosaveEnabled) ? Boolean(object.autosaveEnabled) : false,
        };
    }
    InspectorPreferencesMessage.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.freeCameraInvertRotation !== undefined && (obj.freeCameraInvertRotation = message.freeCameraInvertRotation);
        message.autosaveEnabled !== undefined && (obj.autosaveEnabled = message.autosaveEnabled);
        return obj;
    }
    InspectorPreferencesMessage.toJSON = toJSON;
    function create(base) {
        return InspectorPreferencesMessage.fromPartial(base ?? {});
    }
    InspectorPreferencesMessage.create = create;
    function fromPartial(object) {
        const message = createBaseInspectorPreferencesMessage();
        message.freeCameraInvertRotation = object.freeCameraInvertRotation ?? false;
        message.autosaveEnabled = object.autosaveEnabled ?? false;
        return message;
    }
    InspectorPreferencesMessage.fromPartial = fromPartial;
})(InspectorPreferencesMessage || (exports.InspectorPreferencesMessage = InspectorPreferencesMessage = {}));
function createBaseCopyFileRequest() {
    return { fromPath: "", toPath: "" };
}
var CopyFileRequest;
(function (CopyFileRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.fromPath !== "") {
            writer.uint32(10).string(message.fromPath);
        }
        if (message.toPath !== "") {
            writer.uint32(18).string(message.toPath);
        }
        return writer;
    }
    CopyFileRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseCopyFileRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.fromPath = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.toPath = reader.string();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    CopyFileRequest.decode = decode;
    function fromJSON(object) {
        return {
            fromPath: isSet(object.fromPath) ? String(object.fromPath) : "",
            toPath: isSet(object.toPath) ? String(object.toPath) : "",
        };
    }
    CopyFileRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.fromPath !== undefined && (obj.fromPath = message.fromPath);
        message.toPath !== undefined && (obj.toPath = message.toPath);
        return obj;
    }
    CopyFileRequest.toJSON = toJSON;
    function create(base) {
        return CopyFileRequest.fromPartial(base ?? {});
    }
    CopyFileRequest.create = create;
    function fromPartial(object) {
        const message = createBaseCopyFileRequest();
        message.fromPath = object.fromPath ?? "";
        message.toPath = object.toPath ?? "";
        return message;
    }
    CopyFileRequest.fromPartial = fromPartial;
})(CopyFileRequest || (exports.CopyFileRequest = CopyFileRequest = {}));
function createBaseGetFileRequest() {
    return { path: "" };
}
var GetFileRequest;
(function (GetFileRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        return writer;
    }
    GetFileRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFileRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFileRequest.decode = decode;
    function fromJSON(object) {
        return { path: isSet(object.path) ? String(object.path) : "" };
    }
    GetFileRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        return obj;
    }
    GetFileRequest.toJSON = toJSON;
    function create(base) {
        return GetFileRequest.fromPartial(base ?? {});
    }
    GetFileRequest.create = create;
    function fromPartial(object) {
        const message = createBaseGetFileRequest();
        message.path = object.path ?? "";
        return message;
    }
    GetFileRequest.fromPartial = fromPartial;
})(GetFileRequest || (exports.GetFileRequest = GetFileRequest = {}));
function createBaseGetFileResponse() {
    return { content: new Uint8Array(0) };
}
var GetFileResponse;
(function (GetFileResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.content.length !== 0) {
            writer.uint32(10).bytes(message.content);
        }
        return writer;
    }
    GetFileResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFileResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.content = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFileResponse.decode = decode;
    function fromJSON(object) {
        return { content: isSet(object.content) ? bytesFromBase64(object.content) : new Uint8Array(0) };
    }
    GetFileResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.content !== undefined &&
            (obj.content = base64FromBytes(message.content !== undefined ? message.content : new Uint8Array(0)));
        return obj;
    }
    GetFileResponse.toJSON = toJSON;
    function create(base) {
        return GetFileResponse.fromPartial(base ?? {});
    }
    GetFileResponse.create = create;
    function fromPartial(object) {
        const message = createBaseGetFileResponse();
        message.content = object.content ?? new Uint8Array(0);
        return message;
    }
    GetFileResponse.fromPartial = fromPartial;
})(GetFileResponse || (exports.GetFileResponse = GetFileResponse = {}));
function createBaseCreateCustomAssetRequest() {
    return { name: "", composite: new Uint8Array(0), resources: [], thumbnail: undefined };
}
var CreateCustomAssetRequest;
(function (CreateCustomAssetRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.name !== "") {
            writer.uint32(10).string(message.name);
        }
        if (message.composite.length !== 0) {
            writer.uint32(18).bytes(message.composite);
        }
        for (const v of message.resources) {
            writer.uint32(26).string(v);
        }
        if (message.thumbnail !== undefined) {
            writer.uint32(34).bytes(message.thumbnail);
        }
        return writer;
    }
    CreateCustomAssetRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseCreateCustomAssetRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.name = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.composite = reader.bytes();
                    continue;
                case 3:
                    if (tag !== 26) {
                        break;
                    }
                    message.resources.push(reader.string());
                    continue;
                case 4:
                    if (tag !== 34) {
                        break;
                    }
                    message.thumbnail = reader.bytes();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    CreateCustomAssetRequest.decode = decode;
    function fromJSON(object) {
        return {
            name: isSet(object.name) ? String(object.name) : "",
            composite: isSet(object.composite) ? bytesFromBase64(object.composite) : new Uint8Array(0),
            resources: Array.isArray(object?.resources) ? object.resources.map((e) => String(e)) : [],
            thumbnail: isSet(object.thumbnail) ? bytesFromBase64(object.thumbnail) : undefined,
        };
    }
    CreateCustomAssetRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.name !== undefined && (obj.name = message.name);
        message.composite !== undefined &&
            (obj.composite = base64FromBytes(message.composite !== undefined ? message.composite : new Uint8Array(0)));
        if (message.resources) {
            obj.resources = message.resources.map((e) => e);
        }
        else {
            obj.resources = [];
        }
        message.thumbnail !== undefined &&
            (obj.thumbnail = message.thumbnail !== undefined ? base64FromBytes(message.thumbnail) : undefined);
        return obj;
    }
    CreateCustomAssetRequest.toJSON = toJSON;
    function create(base) {
        return CreateCustomAssetRequest.fromPartial(base ?? {});
    }
    CreateCustomAssetRequest.create = create;
    function fromPartial(object) {
        const message = createBaseCreateCustomAssetRequest();
        message.name = object.name ?? "";
        message.composite = object.composite ?? new Uint8Array(0);
        message.resources = object.resources?.map((e) => e) || [];
        message.thumbnail = object.thumbnail ?? undefined;
        return message;
    }
    CreateCustomAssetRequest.fromPartial = fromPartial;
})(CreateCustomAssetRequest || (exports.CreateCustomAssetRequest = CreateCustomAssetRequest = {}));
function createBaseCreateCustomAssetResponse() {
    return { asset: undefined };
}
var CreateCustomAssetResponse;
(function (CreateCustomAssetResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.asset !== undefined) {
            AssetData.encode(message.asset, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    CreateCustomAssetResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseCreateCustomAssetResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.asset = AssetData.decode(reader, reader.uint32());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    CreateCustomAssetResponse.decode = decode;
    function fromJSON(object) {
        return { asset: isSet(object.asset) ? AssetData.fromJSON(object.asset) : undefined };
    }
    CreateCustomAssetResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.asset !== undefined && (obj.asset = message.asset ? AssetData.toJSON(message.asset) : undefined);
        return obj;
    }
    CreateCustomAssetResponse.toJSON = toJSON;
    function create(base) {
        return CreateCustomAssetResponse.fromPartial(base ?? {});
    }
    CreateCustomAssetResponse.create = create;
    function fromPartial(object) {
        const message = createBaseCreateCustomAssetResponse();
        message.asset = (object.asset !== undefined && object.asset !== null)
            ? AssetData.fromPartial(object.asset)
            : undefined;
        return message;
    }
    CreateCustomAssetResponse.fromPartial = fromPartial;
})(CreateCustomAssetResponse || (exports.CreateCustomAssetResponse = CreateCustomAssetResponse = {}));
function createBaseGetCustomAssetsResponse() {
    return { assets: [] };
}
var GetCustomAssetsResponse;
(function (GetCustomAssetsResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.assets) {
            AssetData.encode(v, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    GetCustomAssetsResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetCustomAssetsResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.assets.push(AssetData.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetCustomAssetsResponse.decode = decode;
    function fromJSON(object) {
        return { assets: Array.isArray(object?.assets) ? object.assets.map((e) => AssetData.fromJSON(e)) : [] };
    }
    GetCustomAssetsResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.assets) {
            obj.assets = message.assets.map((e) => e ? AssetData.toJSON(e) : undefined);
        }
        else {
            obj.assets = [];
        }
        return obj;
    }
    GetCustomAssetsResponse.toJSON = toJSON;
    function create(base) {
        return GetCustomAssetsResponse.fromPartial(base ?? {});
    }
    GetCustomAssetsResponse.create = create;
    function fromPartial(object) {
        const message = createBaseGetCustomAssetsResponse();
        message.assets = object.assets?.map((e) => AssetData.fromPartial(e)) || [];
        return message;
    }
    GetCustomAssetsResponse.fromPartial = fromPartial;
})(GetCustomAssetsResponse || (exports.GetCustomAssetsResponse = GetCustomAssetsResponse = {}));
function createBaseDeleteCustomAssetRequest() {
    return { assetId: "" };
}
var DeleteCustomAssetRequest;
(function (DeleteCustomAssetRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.assetId !== "") {
            writer.uint32(10).string(message.assetId);
        }
        return writer;
    }
    DeleteCustomAssetRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseDeleteCustomAssetRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.assetId = reader.string();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    DeleteCustomAssetRequest.decode = decode;
    function fromJSON(object) {
        return { assetId: isSet(object.assetId) ? String(object.assetId) : "" };
    }
    DeleteCustomAssetRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.assetId !== undefined && (obj.assetId = message.assetId);
        return obj;
    }
    DeleteCustomAssetRequest.toJSON = toJSON;
    function create(base) {
        return DeleteCustomAssetRequest.fromPartial(base ?? {});
    }
    DeleteCustomAssetRequest.create = create;
    function fromPartial(object) {
        const message = createBaseDeleteCustomAssetRequest();
        message.assetId = object.assetId ?? "";
        return message;
    }
    DeleteCustomAssetRequest.fromPartial = fromPartial;
})(DeleteCustomAssetRequest || (exports.DeleteCustomAssetRequest = DeleteCustomAssetRequest = {}));
function createBaseRenameCustomAssetRequest() {
    return { assetId: "", newName: "" };
}
var RenameCustomAssetRequest;
(function (RenameCustomAssetRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.assetId !== "") {
            writer.uint32(10).string(message.assetId);
        }
        if (message.newName !== "") {
            writer.uint32(18).string(message.newName);
        }
        return writer;
    }
    RenameCustomAssetRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseRenameCustomAssetRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.assetId = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.newName = reader.string();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    RenameCustomAssetRequest.decode = decode;
    function fromJSON(object) {
        return {
            assetId: isSet(object.assetId) ? String(object.assetId) : "",
            newName: isSet(object.newName) ? String(object.newName) : "",
        };
    }
    RenameCustomAssetRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.assetId !== undefined && (obj.assetId = message.assetId);
        message.newName !== undefined && (obj.newName = message.newName);
        return obj;
    }
    RenameCustomAssetRequest.toJSON = toJSON;
    function create(base) {
        return RenameCustomAssetRequest.fromPartial(base ?? {});
    }
    RenameCustomAssetRequest.create = create;
    function fromPartial(object) {
        const message = createBaseRenameCustomAssetRequest();
        message.assetId = object.assetId ?? "";
        message.newName = object.newName ?? "";
        return message;
    }
    RenameCustomAssetRequest.fromPartial = fromPartial;
})(RenameCustomAssetRequest || (exports.RenameCustomAssetRequest = RenameCustomAssetRequest = {}));
function createBaseUndoRedoStateResponse() {
    return { canUndo: false, canRedo: false };
}
var UndoRedoStateResponse;
(function (UndoRedoStateResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.canUndo === true) {
            writer.uint32(8).bool(message.canUndo);
        }
        if (message.canRedo === true) {
            writer.uint32(16).bool(message.canRedo);
        }
        return writer;
    }
    UndoRedoStateResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseUndoRedoStateResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.canUndo = reader.bool();
                    continue;
                case 2:
                    if (tag !== 16) {
                        break;
                    }
                    message.canRedo = reader.bool();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    UndoRedoStateResponse.decode = decode;
    function fromJSON(object) {
        return {
            canUndo: isSet(object.canUndo) ? Boolean(object.canUndo) : false,
            canRedo: isSet(object.canRedo) ? Boolean(object.canRedo) : false,
        };
    }
    UndoRedoStateResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.canUndo !== undefined && (obj.canUndo = message.canUndo);
        message.canRedo !== undefined && (obj.canRedo = message.canRedo);
        return obj;
    }
    UndoRedoStateResponse.toJSON = toJSON;
    function create(base) {
        return UndoRedoStateResponse.fromPartial(base ?? {});
    }
    UndoRedoStateResponse.create = create;
    function fromPartial(object) {
        const message = createBaseUndoRedoStateResponse();
        message.canUndo = object.canUndo ?? false;
        message.canRedo = object.canRedo ?? false;
        return message;
    }
    UndoRedoStateResponse.fromPartial = fromPartial;
})(UndoRedoStateResponse || (exports.UndoRedoStateResponse = UndoRedoStateResponse = {}));
function createBaseRemoveFilesRequest() {
    return { filePaths: [] };
}
var RemoveFilesRequest;
(function (RemoveFilesRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.filePaths) {
            writer.uint32(10).string(v);
        }
        return writer;
    }
    RemoveFilesRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseRemoveFilesRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.filePaths.push(reader.string());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    RemoveFilesRequest.decode = decode;
    function fromJSON(object) {
        return { filePaths: Array.isArray(object?.filePaths) ? object.filePaths.map((e) => String(e)) : [] };
    }
    RemoveFilesRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.filePaths) {
            obj.filePaths = message.filePaths.map((e) => e);
        }
        else {
            obj.filePaths = [];
        }
        return obj;
    }
    RemoveFilesRequest.toJSON = toJSON;
    function create(base) {
        return RemoveFilesRequest.fromPartial(base ?? {});
    }
    RemoveFilesRequest.create = create;
    function fromPartial(object) {
        const message = createBaseRemoveFilesRequest();
        message.filePaths = object.filePaths?.map((e) => e) || [];
        return message;
    }
    RemoveFilesRequest.fromPartial = fromPartial;
})(RemoveFilesRequest || (exports.RemoveFilesRequest = RemoveFilesRequest = {}));
function createBaseRemoveFilesResponse() {
    return { success: [], failed: [] };
}
var RemoveFilesResponse;
(function (RemoveFilesResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.success) {
            writer.uint32(10).string(v);
        }
        for (const v of message.failed) {
            writer.uint32(18).string(v);
        }
        return writer;
    }
    RemoveFilesResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseRemoveFilesResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.success.push(reader.string());
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.failed.push(reader.string());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    RemoveFilesResponse.decode = decode;
    function fromJSON(object) {
        return {
            success: Array.isArray(object?.success) ? object.success.map((e) => String(e)) : [],
            failed: Array.isArray(object?.failed) ? object.failed.map((e) => String(e)) : [],
        };
    }
    RemoveFilesResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.success) {
            obj.success = message.success.map((e) => e);
        }
        else {
            obj.success = [];
        }
        if (message.failed) {
            obj.failed = message.failed.map((e) => e);
        }
        else {
            obj.failed = [];
        }
        return obj;
    }
    RemoveFilesResponse.toJSON = toJSON;
    function create(base) {
        return RemoveFilesResponse.fromPartial(base ?? {});
    }
    RemoveFilesResponse.create = create;
    function fromPartial(object) {
        const message = createBaseRemoveFilesResponse();
        message.success = object.success?.map((e) => e) || [];
        message.failed = object.failed?.map((e) => e) || [];
        return message;
    }
    RemoveFilesResponse.fromPartial = fromPartial;
})(RemoveFilesResponse || (exports.RemoveFilesResponse = RemoveFilesResponse = {}));
function createBaseGetFilesListRequest() {
    return { paths: [] };
}
var GetFilesListRequest;
(function (GetFilesListRequest) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.paths) {
            writer.uint32(10).string(v);
        }
        return writer;
    }
    GetFilesListRequest.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesListRequest();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.paths.push(reader.string());
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesListRequest.decode = decode;
    function fromJSON(object) {
        return { paths: Array.isArray(object?.paths) ? object.paths.map((e) => String(e)) : [] };
    }
    GetFilesListRequest.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.paths) {
            obj.paths = message.paths.map((e) => e);
        }
        else {
            obj.paths = [];
        }
        return obj;
    }
    GetFilesListRequest.toJSON = toJSON;
    function create(base) {
        return GetFilesListRequest.fromPartial(base ?? {});
    }
    GetFilesListRequest.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesListRequest();
        message.paths = object.paths?.map((e) => e) || [];
        return message;
    }
    GetFilesListRequest.fromPartial = fromPartial;
})(GetFilesListRequest || (exports.GetFilesListRequest = GetFilesListRequest = {}));
function createBaseGetFilesListResponse() {
    return { files: [] };
}
var GetFilesListResponse;
(function (GetFilesListResponse) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        for (const v of message.files) {
            GetFilesListResponse_FileResult.encode(v, writer.uint32(10).fork()).ldelim();
        }
        return writer;
    }
    GetFilesListResponse.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesListResponse();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.files.push(GetFilesListResponse_FileResult.decode(reader, reader.uint32()));
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesListResponse.decode = decode;
    function fromJSON(object) {
        return {
            files: Array.isArray(object?.files)
                ? object.files.map((e) => GetFilesListResponse_FileResult.fromJSON(e))
                : [],
        };
    }
    GetFilesListResponse.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        if (message.files) {
            obj.files = message.files.map((e) => e ? GetFilesListResponse_FileResult.toJSON(e) : undefined);
        }
        else {
            obj.files = [];
        }
        return obj;
    }
    GetFilesListResponse.toJSON = toJSON;
    function create(base) {
        return GetFilesListResponse.fromPartial(base ?? {});
    }
    GetFilesListResponse.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesListResponse();
        message.files = object.files?.map((e) => GetFilesListResponse_FileResult.fromPartial(e)) || [];
        return message;
    }
    GetFilesListResponse.fromPartial = fromPartial;
})(GetFilesListResponse || (exports.GetFilesListResponse = GetFilesListResponse = {}));
function createBaseGetFilesListResponse_FileResult() {
    return { path: "", content: new Uint8Array(0), success: false, error: undefined };
}
var GetFilesListResponse_FileResult;
(function (GetFilesListResponse_FileResult) {
    function encode(message, writer = minimal_1.default.Writer.create()) {
        if (message.path !== "") {
            writer.uint32(10).string(message.path);
        }
        if (message.content.length !== 0) {
            writer.uint32(18).bytes(message.content);
        }
        if (message.success === true) {
            writer.uint32(24).bool(message.success);
        }
        if (message.error !== undefined) {
            writer.uint32(34).string(message.error);
        }
        return writer;
    }
    GetFilesListResponse_FileResult.encode = encode;
    function decode(input, length) {
        const reader = input instanceof minimal_1.default.Reader ? input : minimal_1.default.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBaseGetFilesListResponse_FileResult();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 10) {
                        break;
                    }
                    message.path = reader.string();
                    continue;
                case 2:
                    if (tag !== 18) {
                        break;
                    }
                    message.content = reader.bytes();
                    continue;
                case 3:
                    if (tag !== 24) {
                        break;
                    }
                    message.success = reader.bool();
                    continue;
                case 4:
                    if (tag !== 34) {
                        break;
                    }
                    message.error = reader.string();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    GetFilesListResponse_FileResult.decode = decode;
    function fromJSON(object) {
        return {
            path: isSet(object.path) ? String(object.path) : "",
            content: isSet(object.content) ? bytesFromBase64(object.content) : new Uint8Array(0),
            success: isSet(object.success) ? Boolean(object.success) : false,
            error: isSet(object.error) ? String(object.error) : undefined,
        };
    }
    GetFilesListResponse_FileResult.fromJSON = fromJSON;
    function toJSON(message) {
        const obj = {};
        message.path !== undefined && (obj.path = message.path);
        message.content !== undefined &&
            (obj.content = base64FromBytes(message.content !== undefined ? message.content : new Uint8Array(0)));
        message.success !== undefined && (obj.success = message.success);
        message.error !== undefined && (obj.error = message.error);
        return obj;
    }
    GetFilesListResponse_FileResult.toJSON = toJSON;
    function create(base) {
        return GetFilesListResponse_FileResult.fromPartial(base ?? {});
    }
    GetFilesListResponse_FileResult.create = create;
    function fromPartial(object) {
        const message = createBaseGetFilesListResponse_FileResult();
        message.path = object.path ?? "";
        message.content = object.content ?? new Uint8Array(0);
        message.success = object.success ?? false;
        message.error = object.error ?? undefined;
        return message;
    }
    GetFilesListResponse_FileResult.fromPartial = fromPartial;
})(GetFilesListResponse_FileResult || (exports.GetFilesListResponse_FileResult = GetFilesListResponse_FileResult = {}));
exports.DataServiceDefinition = {
    name: "DataService",
    fullName: "DataService",
    methods: {
        crdtStream: {
            name: "CrdtStream",
            requestType: CrdtStreamMessage,
            requestStream: true,
            responseType: CrdtStreamMessage,
            responseStream: true,
            options: {},
        },
        undo: {
            name: "Undo",
            requestType: Empty,
            requestStream: false,
            responseType: UndoRedoResponse,
            responseStream: false,
            options: {},
        },
        redo: {
            name: "Redo",
            requestType: Empty,
            requestStream: false,
            responseType: UndoRedoResponse,
            responseStream: false,
            options: {},
        },
        getUndoRedoState: {
            name: "GetUndoRedoState",
            requestType: Empty,
            requestStream: false,
            responseType: UndoRedoStateResponse,
            responseStream: false,
            options: {},
        },
        getFiles: {
            name: "getFiles",
            requestType: GetFilesRequest,
            requestStream: false,
            responseType: GetFilesResponse,
            responseStream: false,
            options: {},
        },
        getFilesSizes: {
            name: "getFilesSizes",
            requestType: GetFilesSizesRequest,
            requestStream: false,
            responseType: GetFilesSizesResponse,
            responseStream: false,
            options: {},
        },
        saveFile: {
            name: "saveFile",
            requestType: SaveFileRequest,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        getAssetCatalog: {
            name: "GetAssetCatalog",
            requestType: Empty,
            requestStream: false,
            responseType: AssetCatalogResponse,
            responseStream: false,
            options: {},
        },
        getAssetData: {
            name: "GetAssetData",
            requestType: Asset,
            requestStream: false,
            responseType: AssetData,
            responseStream: false,
            options: {},
        },
        importAsset: {
            name: "ImportAsset",
            requestType: ImportAssetRequest,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        removeAsset: {
            name: "RemoveAsset",
            requestType: Asset,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        removeFiles: {
            name: "RemoveFiles",
            requestType: RemoveFilesRequest,
            requestStream: false,
            responseType: RemoveFilesResponse,
            responseStream: false,
            options: {},
        },
        save: {
            name: "Save",
            requestType: Empty,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        getInspectorPreferences: {
            name: "GetInspectorPreferences",
            requestType: Empty,
            requestStream: false,
            responseType: InspectorPreferencesMessage,
            responseStream: false,
            options: {},
        },
        setInspectorPreferences: {
            name: "SetInspectorPreferences",
            requestType: InspectorPreferencesMessage,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        copyFile: {
            name: "CopyFile",
            requestType: CopyFileRequest,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        getFile: {
            name: "GetFile",
            requestType: GetFileRequest,
            requestStream: false,
            responseType: GetFileResponse,
            responseStream: false,
            options: {},
        },
        getFilesList: {
            name: "GetFilesList",
            requestType: GetFilesListRequest,
            requestStream: false,
            responseType: GetFilesListResponse,
            responseStream: false,
            options: {},
        },
        createCustomAsset: {
            name: "CreateCustomAsset",
            requestType: CreateCustomAssetRequest,
            requestStream: false,
            responseType: CreateCustomAssetResponse,
            responseStream: false,
            options: {},
        },
        getCustomAssets: {
            name: "GetCustomAssets",
            requestType: Empty,
            requestStream: false,
            responseType: GetCustomAssetsResponse,
            responseStream: false,
            options: {},
        },
        deleteCustomAsset: {
            name: "DeleteCustomAsset",
            requestType: DeleteCustomAssetRequest,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
        renameCustomAsset: {
            name: "RenameCustomAsset",
            requestType: RenameCustomAssetRequest,
            requestStream: false,
            responseType: Empty,
            responseStream: false,
            options: {},
        },
    },
};
const tsProtoGlobalThis = (() => {
    if (typeof globalThis !== "undefined") {
        return globalThis;
    }
    if (typeof self !== "undefined") {
        return self;
    }
    if (typeof window !== "undefined") {
        return window;
    }
    if (typeof global !== "undefined") {
        return global;
    }
    throw "Unable to locate global object";
})();
function bytesFromBase64(b64) {
    if (tsProtoGlobalThis.Buffer) {
        return Uint8Array.from(tsProtoGlobalThis.Buffer.from(b64, "base64"));
    }
    else {
        const bin = tsProtoGlobalThis.atob(b64);
        const arr = new Uint8Array(bin.length);
        for (let i = 0; i < bin.length; ++i) {
            arr[i] = bin.charCodeAt(i);
        }
        return arr;
    }
}
function base64FromBytes(arr) {
    if (tsProtoGlobalThis.Buffer) {
        return tsProtoGlobalThis.Buffer.from(arr).toString("base64");
    }
    else {
        const bin = [];
        arr.forEach((byte) => {
            bin.push(String.fromCharCode(byte));
        });
        return tsProtoGlobalThis.btoa(bin.join(""));
    }
}
function longToNumber(long) {
    if (long.gt(Number.MAX_SAFE_INTEGER)) {
        throw new tsProtoGlobalThis.Error("Value is larger than Number.MAX_SAFE_INTEGER");
    }
    return long.toNumber();
}
if (minimal_1.default.util.Long !== long_1.default) {
    minimal_1.default.util.Long = long_1.default;
    minimal_1.default.configure();
}
function isObject(value) {
    return typeof value === "object" && value !== null;
}
function isSet(value) {
    return value !== null && value !== undefined;
}
