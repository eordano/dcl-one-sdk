import _m0 from "protobufjs/minimal";
/**
 * @public
 */
export interface PBRealmInfo {
    /** the domain of the realm server */
    baseUrl: string;
    /** the name of the realm server (more info https://adr.decentraland.org/adr/ADR-110) */
    realmName: string;
    /** the network id (1=Ethereum, more info https://chainlist.org/) */
    networkId: number;
    /** comms adapter (more info https://adr.decentraland.org/adr/ADR-180) */
    commsAdapter: string;
    /** true if the scene is running as a local preview, instead of published in Decentraland. */
    isPreview: boolean;
    /** the room session id. */
    room?: string | undefined;
    /** true if the user is connected to the scene room. */
    isConnectedSceneRoom?: boolean | undefined;
}
/**
 * @public
 */
export declare namespace PBRealmInfo {
    function encode(message: PBRealmInfo, writer?: _m0.Writer): _m0.Writer;
    function decode(input: _m0.Reader | Uint8Array, length?: number): PBRealmInfo;
}
