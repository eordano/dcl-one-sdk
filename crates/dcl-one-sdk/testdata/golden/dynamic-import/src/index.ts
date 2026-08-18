import { engine, Transform } from '@dcl/sdk/ecs'

// A dynamic import is the one scene shape that makes rolldown want a second
// chunk; `code_splitting: false` inlines it instead, and this fixture is what
// pins that.
export async function main() {
  const { place } = await import('./late')
  place(engine.addEntity(), Transform)
}
