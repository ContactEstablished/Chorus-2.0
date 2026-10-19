/**
 * The 3D startup splash (design/DESIGN.md "Startup splash", spike S7).
 *
 * One particle stream per session being restored, in that session's project colour, sweeps in from
 * depth and condenses into the seven-bar mark; the mark then turns in 3D under bloom. Particle
 * motion runs in the vertex shader, so the CPU does almost nothing per frame. The first frame draws
 * only the background and particles; the mark's materials and the bloom pass compile in the
 * background (`compileAsync`) and join when ready, which keeps the first frame fast.
 */
import * as THREE from 'three'
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js'
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js'
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js'
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js'

// three.js is untyped here (see three.d.ts), so its objects are `any` until P4 adds @types/three.
type Three = any

export interface SplashOptions {
  /** One colour per restoring session (its project colour). */
  streams: string[]
  /** When each session finished restoring, in ms from start; drives when its stream arrives. */
  restoredAtMs: number[]
  accent: string
  greys: [string, string, string]
  background: string
  /** Spike A/B switches. */
  bloom?: boolean
  toneMapping?: boolean
}

export interface SplashMetrics {
  firstFrameMs: number | null
  markReadyMs: number | null
  frames: number
  maxFrameMs: number
  p95FrameMs: number
  doneMs: number | null
  skipped: boolean
  /** Spike: one background pixel at ~600 ms, to check the background colour survives post-processing. */
  backgroundSample?: number[]
}

export interface Splash {
  readonly metrics: SplashMetrics
  skip(): void
  dispose(): void
}

// The mark's geometry (DESIGN.md "Logo"): seven bars, 6 px wide on a 13 px pitch, heights below.
const HEIGHTS = [28, 44, 62, 76, 62, 44, 28]
const TONES = [0, 1, 2, -1, 2, 1, 0] // -1 = accent; 0..2 = grey steps, darkest outside
const UNIT = 0.1 // one mark pixel in world units: the mark is 8.4 × 7.6
const PER_STREAM = 900
const CONVERGE_S = 1.25
const TOTAL_MS = 2800
const FADE_MS = 300

const barX = (i: number) => (13 * i + 3 - 42) * UNIT

const VERTEX = /* glsl */ `
  uniform float uTime;
  uniform float uPixel;
  uniform float uFade;
  attribute vec3 aStart;
  attribute vec3 aCtrl;
  attribute vec3 aColor;
  attribute float aDelay;
  attribute float aSize;
  varying vec3 vColor;
  varying float vAlpha;
  void main() {
    float t = clamp((uTime - aDelay) / ${CONVERGE_S.toFixed(2)}, 0.0, 1.0);
    float e = 1.0 - pow(1.0 - t, 3.0);
    // Each stream flies as a ribbon toward the mark's centre and only spreads into the bars at the
    // end, so the mark reads as condensing out of the streams.
    vec3 flight = mix(mix(aStart, aCtrl, e), mix(aCtrl, vec3(0.0, 0.0, 0.0), e), e);
    vec3 p = mix(flight, position, smoothstep(0.55, 1.0, e));
    float ang = (1.0 - e) * 2.2;
    float c = cos(ang), s = sin(ang);
    p.xy = vec2(c * p.x - s * p.y, s * p.x + c * p.y);
    vec4 mv = modelViewMatrix * vec4(p, 1.0);
    gl_Position = projectionMatrix * mv;
    gl_PointSize = aSize * uPixel * (1.4 + (1.0 - e) * 2.2) * (9.0 / -mv.z);
    vAlpha = smoothstep(0.0, 0.12, t) * (1.0 - uFade);
    vColor = aColor * (1.3 + (1.0 - e) * 1.2);
  }
`

const FRAGMENT = /* glsl */ `
  varying vec3 vColor;
  varying float vAlpha;
  void main() {
    float d = length(gl_PointCoord - 0.5);
    float a = smoothstep(0.5, 0.0, d);
    gl_FragColor = vec4(vColor * a, a * vAlpha);
  }
`

const easeOutCubic = (k: number) => 1 - Math.pow(1 - k, 3)
const easeOutBack = (k: number) => 1 + 2.4 * Math.pow(k - 1, 3) + 1.4 * Math.pow(k - 1, 2)
const clamp01 = (k: number) => Math.min(1, Math.max(0, k))

function roundedBar(height: number): Three {
  const w = 6 * UNIT
  const h = height * UNIT
  const r = w / 2
  const s = new THREE.Shape()
  s.moveTo(-r, -h / 2 + r)
  s.lineTo(-r, h / 2 - r)
  s.absarc(0, h / 2 - r, r, Math.PI, 0, true)
  s.lineTo(r, -h / 2 + r)
  s.absarc(0, -h / 2 + r, r, 0, Math.PI, true)
  const g = new THREE.ExtrudeGeometry(s, { depth: 0.42, bevelEnabled: true, bevelThickness: 0.06, bevelSize: 0.05, bevelSegments: 3, curveSegments: 16 })
  g.center()
  return g
}

/** The page's backdrop (`radial-gradient(ellipse at 50% 45%, #10202c, bg 62%)`) as a texture. */
function backdrop(bg: string): Three {
  const c = document.createElement('canvas')
  c.width = 512
  c.height = 320
  const g = c.getContext('2d')!
  const grad = g.createRadialGradient(256, 144, 0, 256, 144, 0.62 * Math.hypot(256, 176))
  grad.addColorStop(0, '#10202c')
  grad.addColorStop(1, bg)
  g.fillStyle = grad
  g.fillRect(0, 0, c.width, c.height)
  const tex = new THREE.CanvasTexture(c)
  tex.colorSpace = THREE.SRGBColorSpace
  return tex
}

/** Picks a point inside the mark, weighted by bar area, so particles fill it evenly. */
function markPoint(rand: () => number): [number, number, number] {
  const total = HEIGHTS.reduce((a, b) => a + b, 0)
  let pick = rand() * total
  let i = 0
  while (pick > HEIGHTS[i]! && i < HEIGHTS.length - 1) pick -= HEIGHTS[i++]!
  const h = HEIGHTS[i]! * UNIT
  const w = 6 * UNIT
  return [barX(i) + (rand() - 0.5) * w * 0.9, (rand() - 0.5) * h * 0.96, (rand() - 0.5) * 0.4]
}

export function createSplash(canvas: HTMLCanvasElement, opts: SplashOptions, onDone: (m: SplashMetrics) => void): Splash {
  const metrics: SplashMetrics = { firstFrameMs: null, markReadyMs: null, frames: 0, maxFrameMs: 0, p95FrameMs: 0, doneMs: null, skipped: false }
  const frameTimes: number[] = []
  let seed = 7
  const rand = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646

  const width = canvas.clientWidth
  const height = canvas.clientHeight
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: 'high-performance' })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  renderer.setSize(width, height, false)
  // No tone mapping by default: ACES crushed the background darker than the page behind the canvas,
  // so the hand-over from the CSS backdrop to the first WebGL frame showed a jump (S7).
  renderer.toneMapping = opts.toneMapping === true ? THREE.ACESFilmicToneMapping : THREE.NoToneMapping

  const scene = new THREE.Scene()
  // The same radial gradient as the page backdrop, as a texture: scene.background is colour-managed
  // correctly through the bloom composer, whereas renderer.setClearColor came out grey-blue (S7).
  scene.background = backdrop(opts.background)
  scene.fog = new THREE.FogExp2(new THREE.Color(opts.background), 0.01)
  const camera = new THREE.PerspectiveCamera(38, width / height, 0.1, 400)
  camera.position.set(0, 0, 22)
  const group = new THREE.Group()
  group.position.y = 1.15
  scene.add(group)

  // ── Particles: one stream per session, arriving when that session is restored.
  const n = opts.streams.length * PER_STREAM
  const pos = new Float32Array(n * 3)
  const start = new Float32Array(n * 3)
  const ctrl = new Float32Array(n * 3)
  const col = new Float32Array(n * 3)
  const delay = new Float32Array(n)
  const size = new Float32Array(n)
  const tmp = new THREE.Color()
  opts.streams.forEach((hex, s) => {
    tmp.set(hex)
    const angle = (s / opts.streams.length) * Math.PI * 2 + 0.6
    const arrive = Math.max(0, (opts.restoredAtMs[s] ?? 600) / 1000 - CONVERGE_S * 0.75)
    for (let k = 0; k < PER_STREAM; k++) {
      const i = s * PER_STREAM + k
      const [tx, ty, tz] = markPoint(rand)
      pos.set([tx, ty, tz], i * 3)
      const radius = 24 + rand() * 4
      const trail = k / PER_STREAM
      const sx = Math.cos(angle) * radius + (rand() - 0.5) * 3
      const sy = Math.sin(angle) * radius * 0.6 + (rand() - 0.5) * 3
      const sz = -45 - trail * 25 - rand() * 4
      start.set([sx, sy, sz], i * 3)
      // Bend each stream sideways so they sweep in rather than fly straight.
      ctrl.set([(sx + tx) / 2 - Math.sin(angle) * 9, (sy + ty) / 2 + Math.cos(angle) * 9, (sz + tz) / 2 + 12], i * 3)
      const light = 0.75 + rand() * 0.5
      col.set([tmp.r * light, tmp.g * light, tmp.b * light], i * 3)
      delay[i] = arrive + trail * 0.45 + rand() * 0.05
      size[i] = 0.8 + rand() * 1.6
    }
  })
  const geo = new THREE.BufferGeometry()
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3))
  geo.setAttribute('aStart', new THREE.BufferAttribute(start, 3))
  geo.setAttribute('aCtrl', new THREE.BufferAttribute(ctrl, 3))
  geo.setAttribute('aColor', new THREE.BufferAttribute(col, 3))
  geo.setAttribute('aDelay', new THREE.BufferAttribute(delay, 1))
  geo.setAttribute('aSize', new THREE.BufferAttribute(size, 1))
  const particleUniforms = { uTime: { value: 0 }, uPixel: { value: renderer.getPixelRatio() }, uFade: { value: 0 } }
  const points = new THREE.Points(
    geo,
    new THREE.ShaderMaterial({ uniforms: particleUniforms, vertexShader: VERTEX, fragmentShader: FRAGMENT, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }),
  )
  points.frustumCulled = false
  group.add(points)

  // ── The mark and lights, compiled off the critical path.
  const mark = new THREE.Group()
  const markMaterials: Three[] = []
  let accentMaterial: Three = null
  HEIGHTS.forEach((h, i) => {
    const tone = TONES[i]!
    const m =
      tone < 0
        ? new THREE.MeshStandardMaterial({ color: opts.accent, emissive: opts.accent, emissiveIntensity: 0, metalness: 0.2, roughness: 0.25, transparent: true, opacity: 0 })
        : new THREE.MeshStandardMaterial({ color: opts.greys[tone], metalness: 0.55, roughness: 0.32, transparent: true, opacity: 0 })
    if (tone < 0) accentMaterial = m
    markMaterials.push(m)
    const mesh = new THREE.Mesh(roundedBar(h), m)
    mesh.position.x = barX(i)
    mark.add(mesh)
  })
  mark.visible = false
  group.add(mark)
  scene.add(new THREE.AmbientLight(0x8899bb, 0.7))
  const key = new THREE.DirectionalLight(0xffffff, 2.4)
  key.position.set(5, 7, 9)
  scene.add(key)
  const glow = new THREE.PointLight(new THREE.Color(opts.accent), 0, 30)
  glow.position.set(0, 0, 4)
  scene.add(glow)

  let composer: Three = null
  let bloom: Three = null
  const t0 = performance.now()
  void renderer.compileAsync(scene, camera).then(() => {
    metrics.markReadyMs = Math.round(performance.now())
    if (opts.bloom === false) return
    composer = new EffectComposer(renderer)
    composer.addPass(new RenderPass(scene, camera))
    bloom = new UnrealBloomPass(new THREE.Vector2(width, height), 0.6, 0.45, 0.22)
    composer.addPass(bloom)
    composer.addPass(new OutputPass())
    composer.setPixelRatio(renderer.getPixelRatio())
    composer.setSize(width, height)
  })

  // ── Timeline.
  let raf = 0
  let last = 0
  let ended = false
  let skipAt: number | null = null
  const finish = () => {
    if (ended) return
    ended = true
    cancelAnimationFrame(raf)
    const sorted = [...frameTimes].sort((a, b) => a - b)
    metrics.maxFrameMs = Math.round(sorted[sorted.length - 1] ?? 0)
    metrics.p95FrameMs = Math.round(sorted[Math.floor(sorted.length * 0.95)] ?? 0)
    metrics.doneMs = Math.round(performance.now())
    onDone(metrics)
  }
  const frame = (now: number) => {
    const ms = now - t0
    const sec = ms / 1000
    if (last) frameTimes.push(now - last)
    last = now
    particleUniforms.uTime.value = sec

    // Converge, then the mark appears as the particles fade into it.
    const appear = clamp01((ms - 1150) / 500)
    mark.visible = appear > 0 && metrics.markReadyMs !== null
    markMaterials.forEach((m) => (m.opacity = easeOutCubic(appear)))
    particleUniforms.uFade.value = clamp01((ms - 1500) / 600)
    if (accentMaterial) accentMaterial.emissiveIntensity = 1.4 * Math.exp(-Math.pow((ms - 1750) / 420, 2)) + 0.45 * appear
    glow.intensity = 9 * appear
    if (bloom) bloom.strength = 0.45 + 0.75 * Math.exp(-Math.pow((ms - 1750) / 380, 2))

    // The mark turns into place in 3D; the camera eases in.
    const turn = clamp01((ms - 1200) / 1300)
    group.rotation.y = (1 - easeOutBack(turn)) * -Math.PI * 1.25
    group.rotation.x = (1 - easeOutCubic(turn)) * 0.35
    camera.position.z = 26 - 5 * easeOutCubic(clamp01(ms / 2400))

    // Fade out at the end, or quickly on skip.
    const fadeStart = skipAt ?? TOTAL_MS - FADE_MS
    const fadeLen = skipAt === null ? FADE_MS : 180
    canvas.style.opacity = String(1 - clamp01((ms - fadeStart) / fadeLen))

    if (composer) composer.render()
    else renderer.render(scene, camera)
    if (metrics.firstFrameMs === null) metrics.firstFrameMs = Math.round(performance.now())
    if (Math.abs(ms - 600) < 10 && !metrics.backgroundSample) {
      const gl = renderer.getContext()
      const px = new Uint8Array(4)
      gl.readPixels(4, 4, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px)
      metrics.backgroundSample = Array.from(px.slice(0, 3))
    }
    metrics.frames++
    if (ms >= fadeStart + fadeLen) finish()
    else raf = requestAnimationFrame(frame)
  }
  raf = requestAnimationFrame(frame)

  return {
    metrics,
    skip() {
      if (skipAt === null && !ended) {
        metrics.skipped = true
        skipAt = performance.now() - t0
      }
    },
    dispose() {
      cancelAnimationFrame(raf)
      renderer.dispose()
      geo.dispose()
    },
  }
}
