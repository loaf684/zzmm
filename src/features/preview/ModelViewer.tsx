import { useEffect, useMemo } from "react";
import { Canvas } from "@react-three/fiber";
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

interface PartMeshProps {
  part: MeshPart;
  visible: boolean;
  wireframe: boolean;
}

function PartMesh({ part, visible, wireframe }: PartMeshProps) {
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

    built.setIndex(new THREE.BufferAttribute(decodeUint32(part.indices), 1));
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
        // colours are shown unmodified. The neutral clay colour is only the
        // fallback for parts that ship no colour map at all.
        color={hasUvs && texture ? "#ffffff" : "#e8ebf0"}
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
    </mesh>
  );
}

interface ModelViewerProps {
  parts: MeshPart[];
  hiddenParts: string[];
  wireframe: boolean;
  className?: string;
}

export function ModelViewer({
  parts,
  hiddenParts,
  wireframe,
  className,
}: ModelViewerProps) {
  const { center, radius, size } = useMemo(() => boundsOf(parts), [parts]);

  // YZZ/ZZZ position buffers are Z-up, but imported Genshin-style mods keep
  // the Y-up layout. Pick the axis that is clearly the height (with a margin,
  // so wide coats and outstretched arms do not flip a normal Z-up model) and
  // rotate that axis onto Y.
  const upAxis: "y" | "z" = size.y > size.z * 1.3 ? "y" : "z";

  const orientation =
    upAxis === "y"
      ? { rotation: [0, 0, 0] as [number, number, number], position: [-center.x, -center.y, -center.z] as [number, number, number] }
      : { rotation: [-Math.PI / 2, 0, 0] as [number, number, number], position: [-center.x, -center.z, center.y] as [number, number, number] };

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
