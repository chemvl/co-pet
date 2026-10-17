// The pet catalog: the only list of species, skins, palettes, labels, pack flags and pet sizes.
// Pure data (no React); every reader and both art components import from here.
import type { I18nKey } from '../shared/i18n';

export type Species = 'pixel' | 'dog' | 'chicken' | 'pig' | 'owl';
export type PixelSpecies = Exclude<Species, 'pixel'>;
export type Pack = 'free' | 'petPack';
export type SkinId =
  | 'peach' | 'tabby' | 'midnight' | 'cloud' | 'biscuit' | 'cocoa' | 'pebble' | 'cream' | 'russet' | 'lemon'
  | 'pink' | 'mud' | 'ginger' | 'acorn' | 'snowy' | 'dusk';
export interface SpeciesEntry {
  id: Species;
  label: I18nKey;
  pack: Pack;
  /** default first */
  skins: readonly [SkinId, ...SkinId[]];
}
export interface SkinEntry { id: SkinId; species: Species; label: I18nKey; pack: Pack; palette: Readonly<Record<string, string>> }

export const PET_SIZES = [64, 80, 96] as const;
export type PetSize = (typeof PET_SIZES)[number];

/** Palette keys a skin sets per species; the first is the swatch colour. */
export const SKIN_KEYS: Readonly<Record<Species, readonly string[]>> = {
  pixel: ['b', 's', 'l', 'i', 'e'], dog: ['b', 'e', 'm', 'c'], chicken: ['w', 's', 'S', 'r', 'y'],
  pig: ['p', 'P', 'n', 'x'], owl: ['b', 'B', 'f', 'l', 'v', 'y'],
};

export const SPECIES: readonly SpeciesEntry[] = [
  { id: 'pixel', label: 'pet.species.pixel', pack: 'free', skins: ['peach', 'tabby', 'midnight', 'cloud'] },
  { id: 'dog', label: 'pet.species.dog', pack: 'free', skins: ['biscuit', 'cocoa', 'pebble'] },
  { id: 'chicken', label: 'pet.species.chicken', pack: 'free', skins: ['cream', 'russet', 'lemon'] },
  { id: 'pig', label: 'pet.species.pig', pack: 'free', skins: ['pink', 'mud', 'ginger'] },
  { id: 'owl', label: 'pet.species.owl', pack: 'petPack', skins: ['acorn', 'snowy', 'dusk'] },
];
export const SPECIES_IDS: readonly Species[] = SPECIES.map((s) => s.id);
export const PIXEL_SPECIES: readonly PixelSpecies[] = ['dog', 'chicken', 'pig', 'owl'];

export const SKINS: Readonly<Record<SkinId, SkinEntry>> = {
  // Kitty: b body, s stripes, l belly, i inner ear, e iris
  peach:    { id: 'peach', species: 'pixel', label: 'pet.skin.peach', pack: 'free', palette: { b: '#e08a56', s: '#c25f2a', l: '#f6dcc8', i: '#f4c4ae', e: '#fffaf2' } },
  tabby:    { id: 'tabby', species: 'pixel', label: 'pet.skin.tabby', pack: 'petPack', palette: { b: '#b0a69e', s: '#786a61', l: '#f2b8ab', i: '#e99d91', e: '#fffaf2' } },
  midnight: { id: 'midnight', species: 'pixel', label: 'pet.skin.midnight', pack: 'petPack', palette: { b: '#45414f', s: '#45414f', l: '#45414f', i: '#f5a9b8', e: '#f2c14e' } },
  cloud:    { id: 'cloud', species: 'pixel', label: 'pet.skin.cloud', pack: 'petPack', palette: { b: '#f7f4ef', s: '#f7f4ef', l: '#f7f4ef', i: '#f7b3c2', e: '#5fb8f0' } },
  // Puppy: b body, e ears, m muzzle, c collar
  biscuit:  { id: 'biscuit', species: 'dog', label: 'pet.skin.biscuit', pack: 'free', palette: { b: '#f6d2a2', e: '#9b5d3a', m: '#fff4e4', c: '#3fa58f' } },
  cocoa:    { id: 'cocoa', species: 'dog', label: 'pet.skin.cocoa', pack: 'petPack', palette: { b: '#b07a52', e: '#6b4128', m: '#ecd2b4', c: '#e0604f' } },
  pebble:   { id: 'pebble', species: 'dog', label: 'pet.skin.pebble', pack: 'petPack', palette: { b: '#d6d1ca', e: '#6f6862', m: '#f7f4ef', c: '#5b8def' } },
  // Chicken: w body, s wing, S wing shade, r comb, y beak and feet
  cream:    { id: 'cream', species: 'chicken', label: 'pet.skin.cream', pack: 'free', palette: { w: '#fffaf0', s: '#f3dfc4', S: '#e2c39c', r: '#f0605e', y: '#f9b23c' } },
  russet:   { id: 'russet', species: 'chicken', label: 'pet.skin.russet', pack: 'petPack', palette: { w: '#e3a06e', s: '#c97f4f', S: '#a8623a', r: '#e0403e', y: '#f9b23c' } },
  lemon:    { id: 'lemon', species: 'chicken', label: 'pet.skin.lemon', pack: 'petPack', palette: { w: '#ffe58a', s: '#f9cf5a', S: '#e8b23e', r: '#f28c3c', y: '#f07a2a' } },
  // Piggy: p body, P shade, n snout, x nostrils and tail
  pink:     { id: 'pink', species: 'pig', label: 'pet.skin.pink', pack: 'free', palette: { p: '#ffc4cf', P: '#f59bb0', n: '#ffa8ba', x: '#c9607a' } },
  mud:      { id: 'mud', species: 'pig', label: 'pet.skin.mud', pack: 'petPack', palette: { p: '#c99a7b', P: '#a97b5e', n: '#dcb196', x: '#6e4634' } },
  ginger:   { id: 'ginger', species: 'pig', label: 'pet.skin.ginger', pack: 'petPack', palette: { p: '#f2a66e', P: '#d9854c', n: '#f7bd92', x: '#9a4e2a' } },
  // Owl: b body, B dark feathers, f face, l belly, v belly marks, y beak and feet
  acorn:    { id: 'acorn', species: 'owl', label: 'pet.skin.acorn', pack: 'petPack', palette: { b: '#a77b59', B: '#7e5840', f: '#fff3e3', l: '#f4dcb8', v: '#d1a77c', y: '#f6a53a' } },
  snowy:    { id: 'snowy', species: 'owl', label: 'pet.skin.snowy', pack: 'petPack', palette: { b: '#e6e1d9', B: '#b3aa9e', f: '#ffffff', l: '#f6f2ec', v: '#9c9184', y: '#f6a53a' } },
  dusk:     { id: 'dusk', species: 'owl', label: 'pet.skin.dusk', pack: 'petPack', palette: { b: '#6e6a92', B: '#4e4a70', f: '#efe9f7', l: '#d8d0e8', v: '#9a90b8', y: '#f6c03a' } },
};

export function speciesEntry(s: Species): SpeciesEntry {
  return SPECIES.find((e) => e.id === s)!;
}

/** Unknown or legacy values → Kitty. */
export function asSpecies(v: unknown): Species {
  return SPECIES_IDS.find((s) => s === v) ?? 'pixel';
}

export function defaultSkin(s: Species): SkinId {
  return speciesEntry(s).skins[0];
}

export function skinsOf(s: Species): readonly SkinEntry[] {
  return speciesEntry(s).skins.map((id) => SKINS[id]);
}

/** `v` when it is a skin of `s`, else the species' default. Rendering only; never written back. */
export function resolveSkin(s: Species, v: unknown): SkinId {
  return speciesEntry(s).skins.find((id) => id === v) ?? defaultSkin(s);
}

export function swatchOf(id: SkinId): string {
  const skin = SKINS[id];
  return skin.palette[SKIN_KEYS[skin.species][0]];
}

export function asPetSize(v: unknown): PetSize {
  return PET_SIZES.find((s) => s === v) ?? 80;
}

/** `pet.style` + `pet.skin` from the settings map (undefined while loading → Kitty, peach). */
export function petLookOf(settings: Readonly<Record<string, unknown>> | undefined): { species: Species; skin: SkinId } {
  const species = asSpecies(settings?.['pet.style']);
  return { species, skin: resolveSkin(species, settings?.['pet.skin']) };
}
