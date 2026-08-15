'use strict'

// Verbatim ports of the two dump functions from upstream
// `src/lib/data-layer/host/utils/engine-to-composite.ts`.
//
// `dumpEngineToCrdtCommands` is what `dump-crdt` (build step 4/5) runs; it
// needs nothing but an engine. `dumpEngineToComposite` is what `save()` runs.
// The third export of that file upstream, `generateEntityNamesType`, is not
// ported — see index.js.

const ECS = require('@dcl/ecs/dist-cjs')
const { ReadWriteByteBuffer } = require('@dcl/ecs/dist-cjs/serialization/ByteBuffer')

function componentToCompositeComponentData($case, value, component) {
  if ($case === 'json') return { data: { $case, json: value } }
  const byteBuffer = new ReadWriteByteBuffer()
  component.schema.serialize(value, byteBuffer)
  return { data: { $case, binary: byteBuffer.toBinary() } }
}

/**
 * Serialize every component of every entity into a CRDT command stream —
 * the exact bytes a scene's main.crdt holds.
 *
 * @param {import('@dcl/ecs/dist-cjs').IEngine} engine
 * @returns {Uint8Array}
 */
function dumpEngineToCrdtCommands(engine) {
  const componentBuffer = new ReadWriteByteBuffer()
  const crdtBuffer = new ReadWriteByteBuffer()
  for (const itComponentDefinition of engine.componentsIter()) {
    for (const [entity, value] of engine.getEntitiesWith(itComponentDefinition)) {
      if (value) {
        componentBuffer.resetBuffer()
        itComponentDefinition.schema.serialize(value, componentBuffer)
        ECS.PutComponentOperation.write(
          entity,
          0,
          itComponentDefinition.componentId,
          componentBuffer.toBinary(),
          crdtBuffer
        )
      }
    }
  }
  return crdtBuffer.toBinary()
}

/**
 * Turn the live engine back into a composite definition — the inverse of
 * `Composite.instance`, and the whole of what `save()` does.
 *
 * @param {import('@dcl/ecs/dist-cjs').IEngine} engine
 * @param {'json' | 'binary'} internalDataType
 * @returns {import('@dcl/ecs/dist-cjs').Composite.Definition}
 */
function dumpEngineToComposite(engine, internalDataType) {
  const ignoreEntities = new Set()
  /** @type {import('@dcl/ecs/dist-cjs').Composite.Definition} */
  const composite = { version: 1, components: [] }

  const CompositeRoot = ECS.getCompositeRootComponent(engine)
  const childrenComposite = Array.from(engine.getEntitiesWith(CompositeRoot))
  if (childrenComposite.length > 0) {
    const compositeComponent = {
      name: CompositeRoot.componentName,
      jsonSchema: CompositeRoot.schema.jsonSchema,
      data: new Map()
    }
    for (const [compositeRootEntity, compositeRootValue] of childrenComposite) {
      if (compositeRootEntity === engine.RootEntity) continue
      compositeRootValue.entities.forEach((item) => ignoreEntities.add(item.dest))
      compositeComponent.data.set(
        compositeRootEntity,
        componentToCompositeComponentData(
          internalDataType,
          { src: compositeRootValue.src, entities: [] },
          CompositeRoot
        )
      )
    }
    composite.components.push(compositeComponent)
  }

  const ignoreComponentNames = [
    'inspector:Selection',
    'editor::Toggle',
    CompositeRoot.componentName
  ]

  for (const itComponentDefinition of engine.componentsIter()) {
    if (ignoreComponentNames.includes(itComponentDefinition.componentName)) continue
    // APPEND components are not representable in a composite.
    if (itComponentDefinition.componentType === ECS.ComponentType.GrowOnlyValueSet) continue

    const itCompositeComponent = {
      name: itComponentDefinition.componentName,
      jsonSchema: itComponentDefinition.schema.jsonSchema,
      data: new Map()
    }
    for (const [entity, value] of engine.getEntitiesWith(itComponentDefinition)) {
      // Entities that belong to a child composite are that composite's to save.
      if (ignoreEntities.has(entity)) continue
      itCompositeComponent.data.set(
        entity,
        componentToCompositeComponentData(internalDataType, value, itComponentDefinition)
      )
    }
    if (itCompositeComponent.data.size > 0) composite.components.push(itCompositeComponent)
  }
  return composite
}

module.exports = { dumpEngineToCrdtCommands, dumpEngineToComposite }
