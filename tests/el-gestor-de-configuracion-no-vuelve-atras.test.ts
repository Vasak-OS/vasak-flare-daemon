import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * `vasak.conf` es de todo el escritorio, y este demonio es uno de los que lo
 * abre. El gestor de configuración anterior a la 2.6.0 no devolvía el archivo:
 * devolvía su modelo reserializado, y **borraba toda clave que el modelo no
 * conociera** —la disposición de los widgets del escritorio, la pausa del fondo
 * en vídeo—. Como el archivo es compartido, alcanzaba con que **una** aplicación
 * atrasada lo leyera y lo escribiera para dejar sin configuración a las demás.
 *
 * El arreglo entró en la 2.6.0 (`8ee9c00`): cada sección del modelo lleva un
 * `serde(flatten)` con lo que no conoce, así que lo desconocido entra, sale y
 * vuelve al archivo igual que estaba.
 *
 * El rango declarado es `"2"`, que admite volver a una 2.5.x sin que nada falle
 * ni avise. Estas pruebas miran el **bloqueo**, que es lo que se compila.
 */

const LA_VERSION_QUE_ARREGLA = [2, 6, 0] as const;

const raiz = join(import.meta.dir, '..');

function versionBloqueada(nombre: string): string {
	const candado = readFileSync(join(raiz, 'src-tauri', 'Cargo.lock'), 'utf8');
	// Los paquetes van en bloques `[[package]]`; hay que leer la versión del
	// bloque de este nombre y no la del siguiente que aparezca en el archivo.
	const bloque = candado
		.split('[[package]]')
		.find((b) => new RegExp(`^\\s*name = "${nombre}"\\s*$`, 'm').test(b));

	expect(bloque, `${nombre} no está en Cargo.lock`).toBeDefined();

	const version = bloque?.match(/^\s*version = "([^"]+)"\s*$/m)?.[1];
	expect(version, `${nombre} no declara versión`).toBeDefined();

	return version as string;
}

function comparar(version: string): number {
	const partes = version.split('.').map(Number);

	for (const [i, minima] of LA_VERSION_QUE_ARREGLA.entries()) {
		const parte = partes[i] ?? 0;
		if (parte !== minima) return parte - minima;
	}

	return 0;
}

describe('el gestor de configuración que se compila', () => {
	test('no es anterior a la 2.6.0, que es donde dejó de comerse las claves ajenas', () => {
		const version = versionBloqueada('tauri-plugin-config-manager');

		expect(comparar(version)).toBeGreaterThanOrEqual(0);
	});

	test('y la comparación distingue una 2.5.x de una 2.6.0, que es lo que se está cuidando', () => {
		// Sin esto la prueba de arriba pasa aunque `comparar` devuelva siempre 0.
		expect(comparar('2.5.0')).toBeLessThan(0);
		expect(comparar('2.5.9')).toBeLessThan(0);
		expect(comparar('2.6.0')).toBe(0);
		expect(comparar('2.7.0')).toBeGreaterThan(0);
		expect(comparar('3.0.0')).toBeGreaterThan(0);
	});
});
