import { PBAudioAnalysis } from './pb/decentraland/sdk/components/audio_analysis.gen';
/**
 * @internal
 */
export const AudioAnalysisSchema = {
    COMPONENT_ID: 1212,
    serialize(value, builder) {
        const writer = PBAudioAnalysis.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBAudioAnalysis.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBAudioAnalysis.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBAudioAnalysis"
    }
};
