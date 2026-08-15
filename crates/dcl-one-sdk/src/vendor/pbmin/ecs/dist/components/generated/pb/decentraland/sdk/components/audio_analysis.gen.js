/* eslint-disable */
import _m0 from "protobufjs/minimal";
const protobufPackageSarasa = "decentraland.sdk.components";
/**
 * @public
 */
export var PBAudioAnalysisMode;
(function (PBAudioAnalysisMode) {
    PBAudioAnalysisMode[PBAudioAnalysisMode["MODE_RAW"] = 0] = "MODE_RAW";
    PBAudioAnalysisMode[PBAudioAnalysisMode["MODE_LOGARITHMIC"] = 1] = "MODE_LOGARITHMIC";
})(PBAudioAnalysisMode || (PBAudioAnalysisMode = {}));
function createBasePBAudioAnalysis() {
    return {
        mode: 0,
        amplitudeGain: undefined,
        bandsGain: undefined,
        amplitude: 0,
        band0: 0,
        band1: 0,
        band2: 0,
        band3: 0,
        band4: 0,
        band5: 0,
        band6: 0,
        band7: 0,
    };
}
/**
 * @public
 */
export var PBAudioAnalysis;
(function (PBAudioAnalysis) {
    function encode(message, writer = _m0.Writer.create()) {
        if (message.mode !== 0) {
            writer.uint32(8).int32(message.mode);
        }
        if (message.amplitudeGain !== undefined) {
            writer.uint32(805).float(message.amplitudeGain);
        }
        if (message.bandsGain !== undefined) {
            writer.uint32(813).float(message.bandsGain);
        }
        if (message.amplitude !== 0) {
            writer.uint32(1605).float(message.amplitude);
        }
        if (message.band0 !== 0) {
            writer.uint32(1613).float(message.band0);
        }
        if (message.band1 !== 0) {
            writer.uint32(1621).float(message.band1);
        }
        if (message.band2 !== 0) {
            writer.uint32(1629).float(message.band2);
        }
        if (message.band3 !== 0) {
            writer.uint32(1637).float(message.band3);
        }
        if (message.band4 !== 0) {
            writer.uint32(1645).float(message.band4);
        }
        if (message.band5 !== 0) {
            writer.uint32(1653).float(message.band5);
        }
        if (message.band6 !== 0) {
            writer.uint32(1661).float(message.band6);
        }
        if (message.band7 !== 0) {
            writer.uint32(1669).float(message.band7);
        }
        return writer;
    }
    PBAudioAnalysis.encode = encode;
    function decode(input, length) {
        const reader = input instanceof _m0.Reader ? input : _m0.Reader.create(input);
        let end = length === undefined ? reader.len : reader.pos + length;
        const message = createBasePBAudioAnalysis();
        while (reader.pos < end) {
            const tag = reader.uint32();
            switch (tag >>> 3) {
                case 1:
                    if (tag !== 8) {
                        break;
                    }
                    message.mode = reader.int32();
                    continue;
                case 100:
                    if (tag !== 805) {
                        break;
                    }
                    message.amplitudeGain = reader.float();
                    continue;
                case 101:
                    if (tag !== 813) {
                        break;
                    }
                    message.bandsGain = reader.float();
                    continue;
                case 200:
                    if (tag !== 1605) {
                        break;
                    }
                    message.amplitude = reader.float();
                    continue;
                case 201:
                    if (tag !== 1613) {
                        break;
                    }
                    message.band0 = reader.float();
                    continue;
                case 202:
                    if (tag !== 1621) {
                        break;
                    }
                    message.band1 = reader.float();
                    continue;
                case 203:
                    if (tag !== 1629) {
                        break;
                    }
                    message.band2 = reader.float();
                    continue;
                case 204:
                    if (tag !== 1637) {
                        break;
                    }
                    message.band3 = reader.float();
                    continue;
                case 205:
                    if (tag !== 1645) {
                        break;
                    }
                    message.band4 = reader.float();
                    continue;
                case 206:
                    if (tag !== 1653) {
                        break;
                    }
                    message.band5 = reader.float();
                    continue;
                case 207:
                    if (tag !== 1661) {
                        break;
                    }
                    message.band6 = reader.float();
                    continue;
                case 208:
                    if (tag !== 1669) {
                        break;
                    }
                    message.band7 = reader.float();
                    continue;
            }
            if ((tag & 7) === 4 || tag === 0) {
                break;
            }
            reader.skipType(tag & 7);
        }
        return message;
    }
    PBAudioAnalysis.decode = decode;
})(PBAudioAnalysis || (PBAudioAnalysis = {}));
