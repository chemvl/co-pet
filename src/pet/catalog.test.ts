import { describe, expect, it } from 'vitest';
import {
  asPetSize, asSpecies, defaultSkin, petLookOf, resolveSkin, SKIN_KEYS, SKINS, SPECIES,
} from './catalog';

describe('catalog', () => {
  it('asSpecies falls back to Kitty', () => {
    for (const v of ['soft', undefined, 3, 'Dog']) expect(asSpecies(v)).toBe('pixel');
    expect(asSpecies('owl')).toBe('owl');
  });

  it('resolveSkin keeps a skin of the species', () => {
    expect(resolveSkin('dog', 'cocoa')).toBe('cocoa');
  });

  it('resolveSkin falls back to the species default', () => {
    expect(resolveSkin('dog', 'peach')).toBe('biscuit');
    expect(resolveSkin('pixel', 'cocoa')).toBe('peach');
    expect(resolveSkin('owl', undefined)).toBe('acorn');
    expect(resolveSkin('pig', 42)).toBe('pink');
  });

  it('every skin belongs to exactly one species, default first', () => {
    const ids = SPECIES.flatMap((s) => s.skins);
    for (const s of SPECIES) {
      for (const id of s.skins) expect(SKINS[id].species).toBe(s.id);
      expect(defaultSkin(s.id)).toBe(s.skins[0]);
    }
    expect(new Set(ids).size).toBe(ids.length);
    expect([...ids].sort()).toEqual(Object.keys(SKINS).sort());
  });

  it('pack flags follow the owner rule', () => {
    for (const s of SPECIES) {
      expect(s.pack).toBe(s.id === 'owl' ? 'petPack' : 'free');
      for (const id of s.skins) {
        const free = s.id !== 'owl' && id === s.skins[0];
        expect(SKINS[id].pack).toBe(free ? 'free' : 'petPack');
      }
    }
  });

  it('palettes set exactly the species\' skin keys', () => {
    for (const skin of Object.values(SKINS)) {
      expect(Object.keys(skin.palette).sort()).toEqual([...SKIN_KEYS[skin.species]].sort());
      for (const v of Object.values(skin.palette)) expect(v).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it('petLookOf reads settings', () => {
    expect(petLookOf({ 'pet.style': 'dog', 'pet.skin': 'peach' })).toEqual({ species: 'dog', skin: 'biscuit' });
    expect(petLookOf(undefined)).toEqual({ species: 'pixel', skin: 'peach' });
  });

  it('asPetSize', () => {
    expect(asPetSize(96)).toBe(96);
    expect(asPetSize(72)).toBe(80);
    expect(asPetSize('64')).toBe(80);
  });
});
