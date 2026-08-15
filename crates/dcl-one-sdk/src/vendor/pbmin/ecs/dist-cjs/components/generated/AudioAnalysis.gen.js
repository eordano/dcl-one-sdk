"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.AudioAnalysisSchema = void 0;
const audio_analysis_gen_1 = require("./pb/decentraland/sdk/components/audio_analysis.gen");
/**
 * @internal
 */
exports.AudioAnalysisSchema = {
    COMPONENT_ID: 1212,
    serialize(value, builder) {
        const writer = audio_analysis_gen_1.PBAudioAnalysis.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return audio_analysis_gen_1.PBAudioAnalysis.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return audio_analysis_gen_1.PBAudioAnalysis.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAudioAnalysis"
    }
};
