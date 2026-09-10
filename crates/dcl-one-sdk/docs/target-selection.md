# Target selection and Decentraland terminology

Reviewed against the official documentation on 2026-09-09.

The Target page chooses the destination for this project's next publication. The
World and LAND views browse destinations; their explicit selection forms update
the project. They do not publish. **Review deployment** opens the existing upload
review and signing flow. When a delegated signing session is available, opening
the review page waits for the explicit Publish POST; it does not publish on GET.

| Concept | UI behavior | Documentation |
| --- | --- | --- |
| World or LAND | Two destination views: World and LAND (Genesis City). | [Publish your scene](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#publish-your-scene) |
| World identity | A Decentraland NAME or ENS domain identifies the World. A collaborator's deployment grant can be restricted to particular coordinates. Permission to visit is separate from permission to publish. | [Kinds of projects](https://docs.decentraland.org/creator/scenes-sdk7/kinds-of-projects/kinds-of-project), [World collaborators](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#adding-collaborators-to-a-multi-scene-world) |
| Multi-Scene World | Advanced details inside the selected World, not a third destination. The page shows the actual scene layout without inferring a World setting from a scene count. | [Multi-Scene Worlds](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#multi-scene-worlds) |
| Scene placement | A World has its own parcel coordinates. LAND uses Genesis City coordinates. Setting a LAND base shifts the project's whole footprint and requires a later publication. | [Scene metadata](https://docs.decentraland.org/creator/scenes-sdk7/projects/scene-metadata) |
| Overwriting | Every overlapping scene is replaced as a whole, including parcels beyond the new scene's footprint. All overlapping scenes must be marked as replaced in the list and map. | [Scene overwriting](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#scene-overwriting) |
| Server | Display the configured publishing service independently of the World/LAND choice; a custom server is not necessarily the public Genesis City network. | [Custom servers](https://docs.decentraland.org/creator/scene-editor/publish/publish-scene#custom-servers) |
| Returning to LAND | The existing selection handler removes `worldConfiguration` from `scene.json`. | [ADR-173](https://adr.decentraland.org/adr/ADR-173) |
| History | A separate expandable record of past deployments, never a selectable destination. | UI organization based on the two documented destinations. |

The SDK preview already publishes Worlds additively (`multi_scene: true`): it
preserves non-overlapping scenes rather than deleting the whole World first.
The Target page describes that existing behavior. Its advanced section does not
implement Creator Hub's World-settings or collaborator-management controls, and
does not claim that selecting a World converts it to a different mode.

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
to export the renderer's LAND, denied-rights, and World fixtures for browser
review. The fixture values are confined to tests.
