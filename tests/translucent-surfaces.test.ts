import { describe, expect, test } from 'bun:test';
import { Glob } from 'bun';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Los carteles son translúcidos, y sin `backdrop-blur`.
 *
 * La ventana de los carteles es una superficie de capa transparente
 * (`.transparent(true)` y una ventana GTK con visual RGBA): lo que tiene
 * detrás es el escritorio, y el desenfoque lo pone Wayfire. Hasta la 0.4.9 las
 * tarjetas eran `bg-ui-bg/80 backdrop-blur-lg`: el desenfoque del WebView no
 * ve el escritorio —sólo la página, que está vacía detrás de la tarjeta—, así
 * que costaba y no mostraba nada, y al 80 % el texto principal sobre un fondo
 * de pantalla negro daba 4,42:1. Ahora van en `bg-ui-shell`, el fondo de la
 * ventana al 85 % que publica vue-libvasak 2.3.0 (decisión del usuario del
 * 02/10/2026, §13 de `docs/once-ui.md` en la librería).
 *
 * Esta prueba falla si la tarjeta vuelve a un fondo opaco —`bg-ui-float`,
 * `bg-ui-bg` a secas, `bg-ui-surface`—, si pierde el fondo, o si algo de
 * `src/` vuelve a nombrar `backdrop-blur`. Es la misma de vasak-desktop
 * (`tests/translucent-surfaces.test.ts`), sin la excepción de los widgets: acá
 * no hay ninguna superficie que dibuje encima de su propio fondo de pantalla.
 */
const ROOT = join(import.meta.dir, '..');
const read = (file: string) => readFileSync(join(ROOT, file), 'utf8');

/** Saca los comentarios HTML cortando por sus delimitadores. */
function stripHtmlComments(text: string): string {
	let out = '';
	let index = 0;
	while (index < text.length) {
		const start = text.indexOf('<!--', index);
		if (start === -1) return out + text.slice(index);
		out += text.slice(index, start);
		const end = text.indexOf('-->', start + 4);
		if (end === -1) return out;
		index = end + 3;
	}
	return out;
}

const template = (file: string) => {
	const text = read(file);
	return stripHtmlComments(text.slice(text.indexOf('<template>'), text.lastIndexOf('</template>')));
};

/** Los fondos que nombra un trozo de plantilla (`bg-…`), sin variantes de estado. */
function backgroundsOf(classes: string): string[] {
	return [...classes.matchAll(/(?<![\w:/-])bg-([a-z][\w-]*(?:\/\d+)?)(?![\w/-])/g)].map(
		(match) => match[1] as string
	);
}

/** Un fondo deja ver lo de atrás si es `ui-shell`, `transparent` o lleva `/NN` < 100. */
function isTranslucent(background: string): boolean {
	if (background === 'ui-shell' || background === 'transparent') return true;
	const alpha = background.match(/\/(\d+)$/)?.[1];
	return alpha !== undefined && Number(alpha) < 100;
}

/** Lo que mira la prueba, para poder probar la prueba. */
function surfaceProblems(classes: string): string[] {
	const problems: string[] = [];
	const backgrounds = backgroundsOf(classes);
	if (backgrounds.length === 0) problems.push(`sin fondo: «${classes}»`);
	for (const background of backgrounds) {
		if (!isTranslucent(background)) problems.push(`opaco: bg-${background}`);
	}
	if (/backdrop-blur/.test(classes)) problems.push('con backdrop-blur');
	return problems;
}

/** Los archivos de `src/` que nombran `backdrop-blur` fuera de un comentario. */
function filesWithBlur(): string[] {
	return [...new Glob('**/*.{vue,ts,css}').scanSync(join(ROOT, 'src'))]
		.filter((file) => {
			const text = read(`src/${file}`);
			const code = file.endsWith('.vue') ? stripHtmlComments(text) : text;
			return /backdrop-blur/.test(
				code.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*(?:\/\/|\*).*$/gm, '')
			);
		})
		.sort();
}

/** La mezcla de un token de la librería instalada, como `{ base, percent }`. */
function libraryMix(token: string): { base: string; percent: number } | null {
	const tokens = read('node_modules/@vasakgroup/vue-libvasak/dist/tokens.css');
	const value = tokens.match(new RegExp(`--color-${token}:\\s*([^;]+);`))?.[1] ?? '';
	const match = value.match(/^color-mix\(in srgb, var\(--use-([a-z-]+)\) (\d+)%, transparent\)$/);
	return match ? { base: match[1] as string, percent: Number(match[2]) } : null;
}

describe('los carteles dejan ver el desenfoque de Wayfire', () => {
	test('la tarjeta va en bg-ui-shell, sin desenfoque propio', () => {
		const root = template('src/components/NotificationBanner.vue').match(
			/<article\b[\s\S]*?\sclass="([^"]*)"/
		);
		expect(root, 'no se encontró la raíz <article> del cartel').not.toBeNull();
		const classes = (root as RegExpMatchArray)[1] as string;

		expect(surfaceProblems(classes)).toEqual([]);
		expect(backgroundsOf(classes)).toEqual(['ui-shell']);
	});

	test('la pila de App.vue dibuja cada cartel con ese componente', () => {
		// Si alguien vuelve a escribir la tarjeta a mano en App.vue, la prueba
		// de arriba seguiría mirando un componente que ya nadie usa.
		const app = template('src/App.vue');
		expect(app).toContain('<NotificationBanner');
		expect(app).not.toMatch(/<article\b/);
	});

	test('la pila en sí no tiene fondo: entre carteles se ve el escritorio', () => {
		const stack = template('src/App.vue').match(/<div\s+ref="stack"[^>]*class="([^"]*)"/);
		expect(stack, 'no se encontró la pila').not.toBeNull();
		expect(backgroundsOf((stack as RegExpMatchArray)[1] as string)).toEqual([]);
	});

	test('el contador de los que no entran va con el velo translúcido de la insignia', () => {
		// `Badge variant="overlay"`: `ui-overlay`, el fondo al 85 %, que es lo
		// que pide algo que flota sobre un fondo desconocido.
		expect(template('src/App.vue')).toMatch(/<Badge\b[^>]*variant="overlay"/);
	});

	test('ninguna pieza de src/ nombra backdrop-blur', () => {
		expect(filesWithBlur()).toEqual([]);
	});

	test('ui-shell y ui-overlay existen y son translúcidas en la librería instalada', () => {
		for (const token of ['ui-shell', 'ui-overlay']) {
			const mix = libraryMix(token);
			expect(mix, `--color-${token} no es una mezcla con transparent`).not.toBeNull();
			expect(mix?.base).toBe('ui-background');
			expect(mix?.percent).toBeLessThan(100);
		}
	});
});

describe('la guardia de translucidez ve lo opaco cuando lo hay', () => {
	test('rechaza los fondos opacos y el de la 0.4.9', () => {
		expect(surfaceProblems('rounded-corner-l bg-ui-float border border-ui-line')).toEqual([
			'opaco: bg-ui-float',
		]);
		expect(surfaceProblems('flex bg-ui-bg p-3')).toEqual(['opaco: bg-ui-bg']);
		expect(surfaceProblems('bg-ui-surface p-2')).toEqual(['opaco: bg-ui-surface']);
		expect(surfaceProblems('bg-ui-bg/80 p-3 shadow-lg backdrop-blur-lg')).toEqual([
			'con backdrop-blur',
		]);
		expect(surfaceProblems('rounded-corner-l border p-3')).toHaveLength(1);
	});

	test('con cualquier intensidad de desenfoque', () => {
		for (const blur of ['backdrop-blur', 'backdrop-blur-md', 'backdrop-blur-xl']) {
			expect(surfaceProblems(`bg-ui-shell ${blur}`)).toEqual(['con backdrop-blur']);
		}
	});

	test('y deja pasar los translúcidos', () => {
		expect(surfaceProblems('bg-ui-shell shadow-surface-m')).toEqual([]);
		expect(surfaceProblems('bg-ui-surface/70 hover:bg-ui-hover')).toEqual([]);
	});
});
