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
const source = readFileSync(join(import.meta.dir, '..', 'src/components/NotificationBanner.vue'), 'utf8');
const template = source.slice(source.indexOf('<template>')).replace(/<!--[\s\S]*?-->/g, '');

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
