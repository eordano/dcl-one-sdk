"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.isEqual = exports.areConnected = void 0;
/** Returns a string representation of the given coordinates in the format "x,y" */
function formatCoord(coord) {
    return `${coord.x},${coord.y}`;
}
/**
 * Returns true if the given parcels array are connected
 */
function areConnected(parcels) {
    if (parcels.length === 0) {
        return false;
    }
    const visited = visitConnectedParcels(parcels[0], parcels);
    return visited.size === parcels.length;
}
exports.areConnected = areConnected;
/**
 * Returns true if the given coords are equal
 */
function isEqual(p1, p2) {
    return p1.x === p2.x && p1.y === p2.y;
}
exports.isEqual = isEqual;
/**
 * Returns the list of connected parcels starting from the given parcel.
 * @param parcel - The starting parcel to visit
 * @param allParcels - The list of all parcels to consider for connectivity
 * @returns The list of connected parcels starting from the given parcel
 * @remarks This function uses an iterative depth-first search (DFS) approach to avoid blowing the call stack on large connected parcel sets.
 */
function visitConnectedParcels(parcel, allParcels) {
    const allParcelsSet = new Set(allParcels.map(formatCoord));
    const visitedSet = new Set();
    const stackToVisit = [parcel];
    while (stackToVisit.length > 0) {
        const currentParcel = stackToVisit.pop();
        const key = formatCoord(currentParcel);
        const isVisited = visitedSet.has(key);
        if (!isVisited) {
            visitedSet.add(key);
            const neighbours = getNeighbours(currentParcel.x, currentParcel.y, allParcelsSet);
            for (const n of neighbours) {
                if (!visitedSet.has(formatCoord(n)))
                    stackToVisit.push(n);
            }
        }
    }
    return visitedSet;
}
function getNeighbours(x, y, parcels) {
    const neighbourCandidates = [
        { x: x + 1, y },
        { x: x - 1, y },
        { x, y: y + 1 },
        { x, y: y - 1 }
    ];
    return neighbourCandidates.filter((c) => parcels.has(formatCoord(c)));
}
