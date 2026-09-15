# Target selection and Decentraland terminology

Reviewed against the official documentation on 2026-09-09.

The Target page chooses the destination for this project's next publication. The
World and LAND views browse destinations; their explicit selection forms update
the project. They do not publish. **Deploy** opens the existing upload review and
signing flow. With a delegated signing session available, opening the review page
waits for the explicit Publish POST; it does not publish on GET.

| Concept | UI behavior | Documentation |
| --- | --- | --- |
| World or LAND | Two destination views: World and LAND (Genesis City). | [Publish your scene](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#publish-your-scene) |
| World identity | A Decentraland NAME or ENS domain identifies the World. A collaborator's deployment grant can be restricted to particular coordinates. Permission to visit is separate from permission to publish. | [Kinds of projects](https://docs.decentraland.org/creator/scenes-sdk7/kinds-of-projects/kinds-of-project), [World collaborators](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#adding-collaborators-to-a-multi-scene-world) |
| Multi-Scene World | The selected World view shows its scene layout and kept/replaced scenes. It is not a separate destination or tab. | [Multi-Scene Worlds](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#multi-scene-worlds) |
| Scene placement | A World has its own parcel coordinates. LAND uses Genesis City coordinates. Setting a LAND base shifts the project's whole footprint and requires a later publication. | [Scene metadata](https://docs.decentraland.org/creator/scenes-sdk7/projects/scene-metadata) |
| Choosing LAND | Select LAND first to show holdings above the map. Up to six parcels use one table with coordinates, account access, deployed scene and placement action. Larger holdings retain grouped areas and a full parcel inventory. Move here preserves the footprint; placements that cannot fit are unavailable. Estate and incomplete-read limitations are shown. | [Publish your scene](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#publish-your-scene) |
| Overwriting | Every overlapping scene is replaced as a whole, including parcels beyond the new scene's footprint. All overlapping scenes must be marked as replaced in the list and map. | [Scene overwriting](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#scene-overwriting) |
| Server | Display the configured publishing service independently of the World/LAND choice; a custom server is not necessarily the public Genesis City network. | [Custom servers](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#custom-servers) |
| Returning to LAND | The existing selection handler removes `worldConfiguration` from `scene.json`. | [ADR-173](https://adr.decentraland.org/adr/ADR-173) |
| History | Its own tab listing past deployments (age, target, signer, outcome, and the entity / server / HTTP chips of a publish), never a selectable destination. | UI organization based on the two documented destinations; four tabs separate destinations from History and Help. |
| Help | A tab beside History: the publishing guide is its call to action (in the slot the other tabs give Deploy) and the guide anchors this page follows fill its column. It replaced the "Choose where to publish" note above the tab strip. | [Publish your scene](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene) |

The SDK preview publishes Worlds additively by default (`multi_scene: true`).
The World view now shows the scene layout and the separate World entrance
coordinate, read from `GET /world/:name/settings`. Visit links include the
project base parcel, so testing a new scene does not send the author to an older
scene at the World's default entrance. **Make this scene the World entrance**
updates only `spawn_coordinates`, using the owner's browser-wallet signature.
It does not publish the project; the base must already carry a deployed scene.
The action is omitted when visitors already arrive within the scene.

**Replace the entire World with this scene** is an explicit alternative in the
World view. It lists the existing scenes and their coordinates, requires a
checked replacement choice, and checks that the reviewed layout/content still
matches before starting the existing single-scene deployment path. It may ask
for separate publication and removal signatures. Removal precedes upload, so a
failed upload does not restore deleted scenes. Replacement uses the owner's
wallet rather than a delegated publishing session. It is a one-publication
choice; normal publishing remains additive.

Once selected, the World chooser folds under Change World above the layout. Browsing tabs survives background
refreshes, and DCL authorization opens separately so the target page stays open.
The page distinguishes account connection, scene publication, and entrance
updates. Collaborator and other World settings remain in Creator Hub.

The LAND picker also includes all loaded parcels in a table, with owned/operated
roles and deployed scene names. It does not offer a move to an area that cannot
fit the whole footprint. Incomplete deployment reads show unknown occupancy.

Unanswered layout queries are shown as unavailable, not as an empty World. A
listed collaborator grant does not establish permission for every parcel; the
selected footprint's separate rights check still supplies the verdict. Upload
reuse counts describe files available on the server, not the number of semantic
scene changes, so the review link is not labelled "Deploy N changes".

Validation:

```sh
cargo test -p dcl-one-sdk --no-default-features --lib start::
```

Set `DCL_TARGET_DESIGN_CAPTURE` to a temporary directory when running these tests
to export the renderer's LAND, denied-rights and World fixtures for browser
review. The fixture values are confined to tests.

Scene Shape and Target now expose base coordinates directly: type `x,y` and
press Enter (or Move) to translate the whole footprint. The old Change → Pick
on grid mode is removed. Grid painting still edits the footprint shape. Moving
preserves relative spawn points and requires publication to affect the live scene.

The World view uses the selected World name as its heading, with the project
name beneath it. Coordinates appear in the move field; entrance status and
action consequences are stated once beside the relevant control.

Scene permissions describe runtime capabilities, independently of wallet rights
to publish. External media requires a non-empty `allowedMediaHostnames` list
when `ALLOW_MEDIA_HOSTNAMES` is enabled (schemas/src/platform/scene/scene.ts).
The Permissions editor saves both together, clears the list when revoked, and
metadata preparation rejects mismatches before signing or uploading.

A standalone browser publish keeps its HTTP preview running after success or
failure until Ctrl+C. Its completed signing session is retired so the page can
show the result and accept another publish. Publishing from an existing preview
continues to use that server.

## Scene management

The World page leads with the selected name and a concise publish summary. Its
map offers Overview and coordinate focus controls even when scenes are far apart.
Published-scene cards show coordinates, footprint, stored size, entrance and the
publish outcome. Visit and entrance controls stay with their scene; Remove scene
expands a review of that scene's full footprint.

`POST /target/scene/remove` prepares a browser-wallet signature and submits a
single-coordinate World deletion. It checks the local POST gate, selected World,
reviewed layout/content revision, signature age and exact deployment ID before
removal. It never invokes whole-World deletion or uploads a replacement.

The equivalent CLI action is:

```sh
dcl-one-sdk world remove-scene gather.dcl.eth 0,0 --entity <reviewed-entity-id>
```

When retiring the entrance scene, first make the retained scene the entrance.
World settings and scene removal are separate signed server operations.

The Upload card separates transfer files/total and transfer bytes/total from
"On server already" counts. Each row has a byte-based proportion bar. Availability
is not interpreted as proof that the active deployment matches the project.
