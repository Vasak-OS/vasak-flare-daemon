import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Los clics del cartel no se pisan.
 *
 * La tarjeta entera es un botón (la acción por omisión o cerrar), y adentro
 * tiene el de cerrar y los de las acciones. Cada uno de ésos corta su propio
 * clic con `stop-propagation`; si no, tocar «Archivar» ejecutaba además la
 * acción por omisión. Antes lo cortaba la fila con un `@click.stop` en un
 * `<div>`, que es un elemento que escucha el mouse y no el teclado (lo marcó
 * SonarCloud: `Web:MouseEventWithoutKeyboardEquivalentCheck`).
 */
/**
 * Saca los comentarios HTML cortando por sus delimitadores, no con un
 * reemplazo de expresiones regulares: sacar uno puede juntar los pedazos de
 * otro (CodeQL, `js/incomplete-multi-character-sanitization`).
 */
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

const source = readFileSync(join(import.meta.dir, '..', 'src/components/NotificationBanner.vue'), 'utf8');
const template = stripHtmlComments(source.slice(source.indexOf('<template>')));

describe('los botones del cartel', () => {
	const buttons = [...template.matchAll(/<ActionButton\b[\s\S]*?\/>/g)].map((m) => m[0]);

	test('son los dos que se esperan: cerrar y las acciones', () => {
		expect(buttons).toHaveLength(2);
	});

	test('cada uno corta su clic para no disparar el de la tarjeta', () => {
		expect(buttons.filter((button) => !/\sstop-propagation\b/.test(button))).toEqual([]);
	});

	test('y ningún elemento suelto escucha sólo el mouse', () => {
		// Un `@click` en algo que no es la tarjeta ni un componente de la
		// librería es un clic sin equivalente de teclado.
		const plain = [...template.matchAll(/<(div|span|p|img)\b[^>]*@click/g)].map((m) => m[0]);
		expect(plain).toEqual([]);
	});

	test('la tarjeta, que sí escucha el clic, también escucha Enter y Espacio', () => {
		const root = template.match(/<article\b[^>]*>/)?.[0] ?? '';
		expect(root).toContain('@click=');
		expect(root).toContain('@keydown.enter');
		expect(root).toContain('@keydown.space');
	});
});

/**
 * El canto de afuera del cartel es el que eligió la persona.
 *
 * El cartel es un emergente del escritorio, como el panel y el centro de
 * control: su borde de afuera sigue el grosor y el color que se eligen en
 * Configuración (`style.border` de vasak.conf, que el config-manager escribe en
 * `--window-border-width` y `--ui-window-border`). Eso lo trae la utilidad
 * `window-border` de la librería (vue-libvasak 2.16). Con `border
 * border-ui-line` el cartel se quedaba con el canto fino de siempre aunque el
 * resto del escritorio cambiara.
 */
describe('el canto de afuera del cartel', () => {
	const rootClass = template.match(/<article\b[^>]*\sclass="([^"]*)"/)?.[1] ?? '';
	const classes = rootClass.split(/\s+/).filter(Boolean);
	const libraryTokens = readFileSync(
		join(import.meta.dir, '..', 'node_modules/@vasakgroup/vue-libvasak/dist/tokens.css'),
		'utf8'
	);

	test('la tarjeta se leyó', () => {
		// Sin esto, una clase que no se encuentre deja la lista vacía y la
		// prueba de abajo que dice «sin `border-ui-line`» pasa siempre.
		expect(classes).toContain('bg-ui-shell');
	});

	test('el aviso usa el borde que se elige en Configuración (`window-border`)', () => {
		expect(classes).toContain('window-border');
	});

	test('y no el canto fijo de adentro (`border border-ui-line`)', () => {
		expect(classes).not.toContain('border-ui-line');
		expect(classes).not.toContain('border');
	});

	test('la librería instalada declara la utilidad `window-border`', () => {
		// Una clase que Tailwind no conoce no emite nada ni avisa: con una
		// librería anterior a la 2.16 el cartel se quedaría sin borde.
		expect(libraryTokens).toMatch(/@utility\s+window-border\s*\{/);
	});
});
