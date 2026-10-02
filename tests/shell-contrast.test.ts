/**
 * El texto de los carteles se lee sobre cualquier fondo de pantalla.
 *
 * La tarjeta es translúcida (`ui-shell`, el fondo de la ventana al 85 %), así
 * que el color que tiene detrás cada letra depende de lo que haya en el
 * escritorio. Esta prueba compone la superficie sobre **negro y blanco puros**
 * —en sRGB la luminancia de la mezcla crece con la de lo de abajo, así que esos
 * dos son el peor caso para un texto oscuro y para uno claro— y mide contra el
 * 4,5:1 de WCAG 1.4.3 **cada color de texto que usan los carteles**: los que
 * nombran las plantillas (`text-tx-*`) y `tx-main`, que es el de los botones y
 * la insignia de la librería. El contador de los que no entran va sobre
 * `ui-overlay`, y se mide igual.
 *
 * Los porcentajes no se escriben acá: se leen del `tokens.css` de la librería
 * instalada, que es lo que se compila. Los colores son los que escribe el
 * config-manager con los esquemas del sistema (copiados de vue-libvasak en
 * `tests/fixtures/schemes`, junto con el de prueba de acento claro).
 */

import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
	contrast,
	mix,
	type ResolvedPalette,
	resolvePalette,
	type SchemeDocument,
	TEXT_MINIMUM,
} from './scheme';

const ROOT = join(import.meta.dir, '..');
const FIXTURES = join(import.meta.dir, 'fixtures', 'schemes');
const read = (file: string) => readFileSync(join(ROOT, file), 'utf8');

/** Los archivos que dibujan los carteles. */
const BANNER_FILES = ['src/components/NotificationBanner.vue', 'src/App.vue'];

/** De la clase de Tailwind a la variable del esquema que la pinta. */
const TEXT_TOKENS: Record<string, keyof ResolvedPalette> = {
	'tx-main': 'text-main',
	'tx-muted': 'text-muted',
	'tx-on-primary': 'text-on-primary',
	'tx-on-secondary': 'text-on-secondary',
};

/** Los fondos de pantalla del peor caso. */
const WALLPAPERS = { negro: '#000000', blanco: '#ffffff' } as const;

/** La mezcla `color-mix(in srgb, var(--use-X) N%, transparent)` de un token. */
function libraryMix(token: string): { base: keyof ResolvedPalette; percent: number } {
	const tokens = read('node_modules/@vasakgroup/vue-libvasak/dist/tokens.css');
	const value = tokens.match(new RegExp(`--color-${token}:\\s*([^;]+);`))?.[1] ?? '';
	const match = value.match(/^color-mix\(in srgb, var\(--use-([a-z-]+)\) (\d+)%, transparent\)$/);
	if (!match) throw new Error(`--color-${token} no es una mezcla translúcida: «${value}»`);
	return { base: match[1] as keyof ResolvedPalette, percent: Number(match[2]) };
}

/** Los colores de texto que nombran las plantillas de los carteles. */
function textTokensUsed(): string[] {
	const found = new Set<string>(['tx-main']);
	for (const file of BANNER_FILES) {
		const text = read(file);
		const template = text.slice(text.indexOf('<template>'));
		for (const match of template.matchAll(/(?<![\w-])text-(tx-[a-z-]+)(?:\/\d+)?(?![\w-])/g)) {
			found.add(match[1] as string);
		}
	}
	return [...found].sort();
}

const schemes: SchemeDocument[] = readdirSync(FIXTURES)
	.filter((file) => file.endsWith('.json'))
	.sort()
	.map((file) => JSON.parse(readFileSync(join(FIXTURES, file), 'utf8')) as SchemeDocument);

const surfaces = { 'ui-shell': libraryMix('ui-shell'), 'ui-overlay': libraryMix('ui-overlay') };
const used = textTokensUsed();

describe('lo que se mide se leyó', () => {
	test('los esquemas, las mezclas y los colores de texto', () => {
		// Sin esto, una carpeta vacía o una plantilla que se mudó dejan la
		// prueba de abajo midiendo nada, y pasa.
		expect(schemes.map((scheme) => scheme.id)).toContain('vasak-default');
		expect(surfaces['ui-shell'].base).toBe('ui-background');
		expect(surfaces['ui-shell'].percent).toBeLessThan(100);
		expect(used).toContain('tx-main');
		for (const token of used) expect(TEXT_TOKENS[token], `${token} sin variable`).toBeDefined();
	});

	test('la composición da lo que dice la especificación', () => {
		// §13 de `docs/once-ui.md`: al 80 % daba 4,42:1 y al 85 %, 5,01:1, con
		// el esquema de fábrica en claro sobre negro puro.
		const light = resolvePalette((schemes.find((s) => s.id === 'vasak-default') as SchemeDocument).colors.light);
		const at = (percent: number) =>
			contrast(light['text-main'], mix(light['ui-background'], percent, WALLPAPERS.negro));
		expect(at(80)).toBeLessThan(TEXT_MINIMUM);
		expect(at(85)).toBeCloseTo(5.01, 2);
	});
});

for (const scheme of schemes) {
	for (const mode of ['light', 'dark'] as const) {
		const palette = resolvePalette(scheme.colors[mode]);
		describe(`${scheme.id}, ${mode === 'light' ? 'claro' : 'oscuro'}`, () => {
			for (const [surface, { base, percent }] of Object.entries(surfaces)) {
				test(`cada texto de los carteles llega a 4,5:1 sobre ${surface}, con cualquier fondo de pantalla`, () => {
					const short: string[] = [];
					for (const [wallpaper, color] of Object.entries(WALLPAPERS)) {
						const behind = mix(palette[base], percent, color);
						for (const token of used) {
							const ratio = contrast(palette[TEXT_TOKENS[token] as keyof ResolvedPalette], behind);
							if (ratio < TEXT_MINIMUM) short.push(`${token} sobre fondo ${wallpaper}: ${ratio.toFixed(2)}`);
						}
					}
					expect(short).toEqual([]);
				});
			}
		});
	}
}

describe('la medida ve un texto que no llega cuando lo hay', () => {
	test('tx-muted en claro sobre un fondo de pantalla negro no llega', () => {
		// Es por qué los carteles no usan `tx-muted`: si alguien lo vuelve a
		// poner, la prueba de arriba lo agarra porque lo lee de la plantilla.
		const light = resolvePalette((schemes.find((s) => s.id === 'vasak-default') as SchemeDocument).colors.light);
		const { base, percent } = surfaces['ui-shell'];
		expect(contrast(light['text-muted'], mix(light[base], percent, WALLPAPERS.negro))).toBeLessThan(
			TEXT_MINIMUM
		);
	});
});
