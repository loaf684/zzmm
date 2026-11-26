import { useEffect, useMemo } from "react";
import { Canvas, useThree } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import * as THREE from "three";

import { cn } from "@/lib/cn";
import type { MeshPart } from "@/types/preview";

/** Decode base64 little-endian binary into typed arrays without copying twice. */
function decodeFloat32(base64: string): Float32Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return new Float32Array(bytes.buffer);
}

function decodeUint32(base64: string): Uint32Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return new Uint32Array(bytes.buffer);
}

interface Bounds {
  center: THREE.Vector3;
  radius: number;
  /** Extent of the combined mesh on each local axis. */
  size: THREE.Vector3;
}

function boundsOf(parts: MeshPart[]): Bounds {
  const min = new THREE.Vector3(Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY);
  const max = new THREE.Vector3(Number.NEGATIVE_INFINITY, Number.NEGATIVE_INFINITY, Number.NEGATIVE_INFINITY);

  for (const part of parts) {
    const positions = decodeFloat32(part.positions);
    for (let index = 0; index + 2 < positions.length; index += 3) {
      const x = positions[index];
      const y = positions[index + 1];
      const z = positions[index + 2];
      // A corrupt buffer can carry NaN or infinities; one such vertex would
      // otherwise blow up the bounds and leave the viewport empty.
      if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) {
        continue;
      }
      if (min.x > x) min.x = x;
      if (min.y > y) min.y = y;
      if (min.z > z) min.z = z;
      if (max.x < x) max.x = x;
      if (max.y < y) max.y = y;
      if (max.z < z) max.z = z;
    }
  }

  if (!Number.isFinite(min.x) || !Number.isFinite(max.x)) {
    return {
      center: new THREE.Vector3(0, 0, 0),
      radius: 1,
      size: new THREE.Vector3(1, 1, 1),
    };
  }

  const size = max.clone().sub(min);
  const center = min.clone().add(max).multiplyScalar(0.5);
  const radius = Math.max(size.length() * 0.5, 0.001);

  return { center, radius, size };
}

/**
 * Drop triangles whose indices point past the vertex array. A mis-decoded index
 * buffer would otherwise make WebGL draw garbage or throw on the whole part.
 */
function sanitizeIndices(indices: Uint32Array, vertexCount: number): Uint32Array {
  let outOfRange = false;
  for (const value of indices) {
    if (value >= vertexCount) {
      outOfRange = true;
      break;
    }
  }

  if (!outOfRange) return indices;

  const kept: number[] = [];
  for (let triangle = 0; triangle + 2 < indices.length; triangle += 3) {
    const a = indices[triangle];
    const b = indices[triangle + 1];
    const c = indices[triangle + 2];
    if (a < vertexCount && b < vertexCount && c < vertexCount) {
      kept.push(a, b, c);
    }
  }

  return Uint32Array.from(kept);
}

/**
 * Ink contour. A back-face shell pushed out along the vertex normals: the model
 * gains the game's thin outline without a second post-processing pass, and the
 * mesh itself is untouched.
 *
 * The push happens in clip space, so `thickness` is a hairline of CSS pixels
 * rather than a world-space shell that would swamp a small prop and vanish on a
 * large one.
 */
const OUTLINE_COLOR = "#2a3346";
const OUTLINE_OPACITY = 0.85;
const OUTLINE_THICKNESS = 1.6;
/** Matches the surface's `alphaTest` for cutout parts (hair, lace). */
const OUTLINE_ALPHA_TEST = 0.5;

const OUTLINE_VERTEX_SHADER = /* glsl */ `
  uniform float thickness;
  uniform vec2 resolution;
  varying vec2 vUv;

  void main() {
    vUv = uv;

    vec4 clipPosition = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    vec4 clipNormal = projectionMatrix * modelViewMatrix * vec4(normal, 0.0);
    vec2 direction = clipNormal.xy;

    // A vertex whose normal points at the camera projects to zero length;
    // normalising that would poison the whole triangle with NaN. Such a face is
    // already covered by the surface it backs, so leaving it in place is free.
    if (dot(direction, direction) > 1e-8) {
      // offset_ndc = direction * thickness / resolution * 2, so after the
      // viewport divide the contour is one hairline wide whatever the canvas
      // size or the camera distance.
      clipPosition.xy += normalize(direction) * thickness / resolution * clipPosition.w * 2.0;
    }

    gl_Position = clipPosition;
  }
`;

const OUTLINE_FRAGMENT_SHADER = /* glsl */ `
  uniform vec3 color;
  uniform float opacity;
  uniform sampler2D map;
  uniform float hasMap;
  uniform float alphaTest;
  varying vec2 vUv;

  void main() {
    // Cutout parts must drop the shell's transparent texels too, otherwise the
    // outline would trace every hair card's quad instead of the hair.
    if (hasMap > 0.5 && texture2D(map, vUv).a < alphaTest) discard;

    gl_FragColor = vec4(color, opacity);
    #include <tonemapping_fragment>
    #include <colorspace_fragment>
  }
`;

interface PartOutlineProps {
  geometry: THREE.BufferGeometry;
  visible: boolean;
  /** The part's diffuse, so cutout cards keep their mask. */
  map: THREE.Texture | null;
  hasMap: boolean;
  alphaTest: number;
}

function PartOutline({
  geometry,
  visible,
  map,
  hasMap,
  alphaTest,
}: PartOutlineProps) {
  // The shader converts pixels to clipping offsets, so it needs the canvas size
  // and has to follow every resize (docking and expanding the preview).
  const size = useThree((state) => state.size);

  const material = useMemo(
    () =>
      new THREE.ShaderMaterial({
        // Only the shell's far side is drawn, which is what turns the offset
        // copy into a rim instead of a silhouette around the whole model.
        side: THREE.BackSide,
        transparent: true,
        // Coplanar shells (a flat card facing the camera) must not fight the
        // surface for the same depth, and the rim of one part should never
        // occlude the rim of the next.
        depthFunc: THREE.LessDepth,
        depthWrite: false,
        // The shell shares the surface's depth; where a mask opens a hole (hair,
        // lace) or the camera sees a part's inside, the two would otherwise
        // z-fight into moire. Bias the shell behind by a fraction of a pixel so
        // the surface always wins a tie.
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
        uniforms: {
          color: { value: new THREE.Color(OUTLINE_COLOR) },
          opacity: { value: OUTLINE_OPACITY },
          thickness: { value: OUTLINE_THICKNESS },
          resolution: { value: new THREE.Vector2(1, 1) },
          map: { value: null },
          hasMap: { value: 0 },
          alphaTest: { value: 0 },
        },
        vertexShader: OUTLINE_VERTEX_SHADER,
        fragmentShader: OUTLINE_FRAGMENT_SHADER,
      }),
    [],
  );

  useEffect(() => () => material.dispose(), [material]);

  useEffect(() => {
    const uniforms = material.uniforms;
    (uniforms.resolution.value as THREE.Vector2).set(size.width, size.height);
    uniforms.map.value = hasMap ? map : null;
    uniforms.hasMap.value = hasMap ? 1 : 0;
    uniforms.alphaTest.value = hasMap ? alphaTest : 0;
  }, [material, size.width, size.height, map, hasMap, alphaTest]);

  return <mesh geometry={geometry} material={material} visible={visible} />;
}

interface PartMeshProps {
  part: MeshPart;
  visible: boolean;
  wireframe: boolean;
  /** The caller turns this off in wireframe mode: a filled shell hides the wires. */
  outline: boolean;
}

function PartMesh({ part, visible, wireframe, outline }: PartMeshProps) {
  // Rust leaves `uvs` empty when no texcoord buffer matched the mesh; mapping
  // the texture anyway would paint the whole part with a single texel.
  const hasUvs = part.uvs.length > 0;

  const geometry = useMemo(() => {
    const built = new THREE.BufferGeometry();

    const positions = decodeFloat32(part.positions);
    built.setAttribute("position", new THREE.BufferAttribute(positions, 3));

    const normals = decodeFloat32(part.normals);
    if (normals.length === part.vertexCount * 3) {
      built.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
    } else {
      // Rust always sends normals; this is a belt-and-braces fallback.
      built.computeVertexNormals();
    }

    if (part.uvs) {
      const uvs = decodeFloat32(part.uvs);
      if (uvs.length === part.vertexCount * 2) {
        built.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
      }
    }

    const vertexCount = Math.floor(positions.length / 3);
    const indices = sanitizeIndices(decodeUint32(part.indices), vertexCount);
    built.setIndex(new THREE.BufferAttribute(indices, 1));
    built.computeBoundingSphere();

    return built;
  }, [part]);

  useEffect(() => () => geometry.dispose(), [geometry]);

  const texture = useMemo(() => {
    if (!part.texture) return null;

    const loaded = new THREE.TextureLoader().load(part.texture);
    loaded.colorSpace = THREE.SRGBColorSpace;
    // D3D textures and 3DMigoto UVs both use a top-left origin.
    loaded.flipY = false;
    loaded.anisotropy = 4;
    // Game shaders sample with wrap addressing: hair, cloth and scales use UVs
    // that tile outside 0..1. Three's default clamp would smear the last texel
    // across those triangles, which reads as bright edge streaks.
    loaded.wrapS = THREE.RepeatWrapping;
    loaded.wrapT = THREE.RepeatWrapping;

    return loaded;
  }, [part.texture]);

  useEffect(() => () => texture?.dispose(), [texture]);

  return (
    <mesh geometry={geometry} visible={visible}>
      <meshStandardMaterial
        map={hasUvs ? texture ?? undefined : undefined}
        // With a diffuse texture the base colour stays white so the map's own
        // colours are shown unmodified. A part with no colour map is drawn in a
        // clearly darker neutral grey: bright clay used to read as a failed
        // texture load, and the panel now names the count instead.
        color={hasUvs && texture ? "#ffffff" : "#aeb5c0"}
        roughness={0.68}
        metalness={0.03}
        wireframe={wireframe}
        // ZZZ hair, leaves and cloth are drawn with a masked shader. Alpha
        // testing reproduces the cutout; without it those cards render opaque
        // and read as overlapping geometry. Opaque textures keep alphaTest 0.
        alphaTest={part.hasAlpha && hasUvs ? 0.5 : 0}
        // ZZZ meshes are not consistently wound, so render both faces.
        side={THREE.DoubleSide}
      />
      {outline ? (
        <PartOutline
          geometry={geometry}
          visible={visible}
          map={hasUvs ? texture : null}
          hasMap={hasUvs && texture !== null}
          alphaTest={OUTLINE_ALPHA_TEST}
        />
      ) : null}
    </mesh>
  );
}

interface ModelViewerProps {
  parts: MeshPart[];
  hiddenParts: string[];
  wireframe: boolean;
  outline: boolean;
  className?: string;
}

export function ModelViewer({
  parts,
  hiddenParts,
  wireframe,
  outline,
  className,
}: ModelViewerProps) {
  // Frame the camera and pick the up axis from the parts that carry the model,
  // so a stray prop (an untextured cloak, a bone-parented accessory authored in
  // another orientation) cannot shrink the character or tip it over.
  const framing = useMemo(() => {
    if (parts.length <= 1) return parts;

    const largest = parts.reduce(
      (max, part) => Math.max(max, part.triangleCount),
      0,
    );
    const threshold = Math.max(largest * 0.02, 1);
    const main = parts.filter((part) => part.triangleCount >= threshold);

    return main.length > 0 ? main : parts;
  }, [parts]);

  const { center, radius, size } = useMemo(() => boundsOf(framing), [framing]);

  // YZZ/ZZZ position buffers are Z-up, but imported Genshin-style mods keep
  // the Y-up layout. Pick the axis that is clearly the height (with a margin,
  // so wide coats and outstretched arms do not flip a normal Z-up model) and
  // rotate that axis onto Y.
  const upAxis: "y" | "z" = size.y > size.z * 1.3 ? "y" : "z";

  const orientation =
    upAxis === "y"
      ? { rotation: [0, 0, 0] as [number, number, number], position: [-center.x, -center.y, -center.z] as [number, number, number] }
      : { rotation: [-Math.PI / 2, 0, 0] as [number, number, number], position: [-center.x, -center.z, center.y] as [number, number, number] };

  // Wireframe draws the surface as edges only, so it writes depth along those
  // edges and nowhere else. The back-face shell is a *filled* surface: with the
  // outline on, everything between the wires is covered by near-opaque ink and
  // the model reads as a solid silhouette. Skip the shell in wireframe mode so
  // the wires stay see-through; the `outline` preference itself is untouched.
  const showOutline = outline && !wireframe;

  return (
    <div
      className={cn(
        "relative h-[560px] w-full border border-ink-700/90 bg-gradient-to-b from-ink-900 to-ink-950",
        className,
      )}
    >
      <Canvas
        key={`${radius}-${upAxis}`}
        dpr={[1, 2]}
        gl={{ antialias: true, alpha: false }}
        camera={{
          position: [radius * 1.7, radius * 0.9, radius * 2.1],
          fov: 38,
          near: Math.max(radius / 1000, 0.001),
          far: Math.max(radius * 60, 10),
        }}
        onCreated={({ gl }) => {
          gl.setClearColor("#0a0c12", 1);
          gl.outputColorSpace = THREE.SRGBColorSpace;
          gl.toneMapping = THREE.ACESFilmicToneMapping;
          // Texture maps now render at full albedo (no grey tint), so the
          // exposure is pulled back to keep the same overall brightness and
          // stop bright cloth/hair from clipping to white.
          gl.toneMappingExposure = 0.72;
        }}
      >
        <color attach="background" args={["#0a0c12"]} />
        <ambientLight intensity={1.05} />
        <hemisphereLight args={["#cfe4ff", "#1a1d26", 1.15]} />
        <directionalLight
          position={[radius * 4, radius * 6, radius * 3]}
          intensity={2.6}
        />
        <directionalLight
          position={[-radius * 5, radius * 2, -radius * 3]}
          intensity={1.1}
        />
        <directionalLight
          position={[0, -radius * 4, radius * 2]}
          intensity={0.55}
        />

        {/* World = T · R · local; the translation moves the rotated centre to
            the origin so orbiting stays centred on the character. */}
        <group position={orientation.position} rotation={orientation.rotation}>
          {parts.map((part, index) => (
            <PartMesh
              key={`${part.name}-${index}`}
              part={part}
              visible={!hiddenParts.includes(part.name)}
              wireframe={wireframe}
              outline={showOutline}
            />
          ))}
        </group>

        <OrbitControls
          makeDefault
          enableDamping
          dampingFactor={0.08}
          target={[0, 0, 0]}
          minDistance={radius * 0.35}
          maxDistance={radius * 12}
        />
      </Canvas>
    </div>
  );
}
