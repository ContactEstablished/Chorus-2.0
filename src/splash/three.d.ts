// `three` ships no TypeScript types and `@types/three` is not on the approved dependency list
// (ARCHITECTURE §10), so the splash treats three.js as untyped for now. P4 should ask to add
// `@types/three`.
declare module 'three'
declare module 'three/examples/jsm/*'
