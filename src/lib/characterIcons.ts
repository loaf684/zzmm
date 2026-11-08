// Character portraits are the "Agent … Circle Icon" files from the Zenless
// Zone Zero Wiki (https://zenless-zone-zero.fandom.com). They are bundled so
// the sidebar works offline; unknown or custom names fall back to an initial.

const FILES = import.meta.glob("../assets/characters/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const ICONS = new Map<string, string>();
for (const [path, url] of Object.entries(FILES)) {
  const slug = path.slice(path.lastIndexOf("/") + 1, path.lastIndexOf("."));
  ICONS.set(slug, url);
}

/** Alternate spellings and short forms that the roster can produce. */
const ALIASES: Record<string, string> = {
  "alexandrina-sebastiane": "rina",
  "anby-s0": "soldier-0-anby",
  "s0-anby": "soldier-0-anby",
  "soldier-0": "soldier-0-anby",
  s11: "soldier-11",
  ellen: "ellen-joe",
  zhuyuan: "zhu-yuan",
  nicole: "nicole-demara",
  anby: "anby-demara",
  billy: "billy-kid",
  corin: "corin-wickes",
  caesar: "caesar-king",
  burnice: "burnice-white",
  jane: "jane-doe",
  seth: "seth-lowell",
  grace: "grace-howard",
  lycaon: "von-lycaon",
  koleda: "koleda-belobog",
  ben: "ben-bigger",
  anton: "anton-ivanov",
  astra: "astra-yao",
  evelyn: "evelyn-chevalier",
  miyabi: "hoshimi-miyabi",
  harumasa: "asaba-harumasa",
  yanagi: "tsukishiro-yanagi",
  vivian: "vivian-banshee",
  hugo: "hugo-vlad",
  "yi-xuan": "yixuan",
  jufufu: "ju-fufu",
  alice: "alice-thymefield",
  pulchra: "pulchra-fellini",
  "orphie-magnusson-magus": "orphie-magus",
  "lucia-elowen": "lucia",
  "yidhari-murphy": "yidhari",
  "komano-manato": "manato",
  "pan-yinhu": "pan-yinhu",
  piper: "piper-wheel",
  "luciana-de-montefio": "lucy",
  "nekomiya-mana": "nekomata",
  "ukinami-yuzuha": "yuzuha",
  cissia: "cissia",
  "claret-flint": "claret",
  "nangong-yu": "nangong",
  "norma-hollowell": "norma",
  "phoenix-reffaella": "phoenix",
  "remielle-dan": "remielle",
  "roxy-ifrita-pryce": "roxi",
  roxy: "roxi",
  "sigrid-de-lazur": "sigrid",
  "velina-airgid": "velina",
  yeshunguang: "ye-shunguang",
};

function normalize(value: string): string {
  return value
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/['’`]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** Bundled circle icon for a character name, or null when none is known. */
export function characterIcon(character: string): string | null {
  const key = normalize(character);
  return ICONS.get(ALIASES[key] ?? key) ?? null;
}
