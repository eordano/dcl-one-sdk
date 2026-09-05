import { PBExplorerUiEventsResult } from './pb/decentraland/sdk/components/explorer_ui_events_result.gen';
/**
 * @internal
 */
export const ExplorerUiEventsResultSchema = {
    COMPONENT_ID: 1220,
    serialize(value, builder) {
        const writer = PBExplorerUiEventsResult.encode(value);
        const buffer = new Uint8Array(writer.finish(), 0, writer.len);
        builder.writeBuffer(buffer, false);
    },
    deserialize(reader) {
        return PBExplorerUiEventsResult.decode(reader.buffer(), reader.remainingBytes());
    },
    create() {
        // TODO: this is a hack.
        return PBExplorerUiEventsResult.decode(new Uint8Array());
    },
    jsonSchema: {
        type: "object",
        properties: {},
        serializationType: "protocol-buffer",
        protocolBuffer: "PBExplorerUiEventsResult"
    }
};
