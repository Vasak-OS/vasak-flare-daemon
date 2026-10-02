import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Las dependencias del `.deb` son las que enlaza el binario.
 *
 * Hasta la 0.4.9 la lista era la de la plantilla `vapp`, igual en once
 * aplicaciones y sin relación con lo que enlaza cada una: traía `libsoup2.4-1`
 * **y** `libsoup-3.0-0` (las dos generaciones a la vez) y `libpango-1.0-0`, que
 * el demonio no enlaza, y le faltaban cuatro que sí: la capa de Wayland, la
 * base de las notificaciones, D-Bus y JavaScriptCore. Sin ésas, el paquete se
 * instala y el demonio no arranca.
 *
 * Auditado con `readelf -d … | grep NEEDED` sobre el binario de la 0.5.0
 * (nunca con `ldd`, que suma las transitivas). Si el binario empieza a enlazar
 * otra biblioteca, esta lista y la receta de Arch suben juntas.
 */
const conf = JSON.parse(
	readFileSync(join(import.meta.dir, '..', 'src-tauri', 'tauri.conf.json'), 'utf8')
) as { bundle: { linux: { deb: { depends: string[] } } } };
const depends = conf.bundle.linux.deb.depends;

/** Cada biblioteca que figura como NEEDED, con el paquete de Debian que la trae. */
const LINKED: Record<string, string> = {
	'libgtk-layer-shell.so.0': 'libgtk-layer-shell0',
	'libsqlite3.so.0': 'libsqlite3-0',
	'libgdk-3.so.0': 'libgtk-3-0t64',
	'libgtk-3.so.0': 'libgtk-3-0t64',
	'libgdk_pixbuf-2.0.so.0': 'libgdk-pixbuf-2.0-0',
	'libcairo.so.2': 'libcairo2',
	'libglib-2.0.so.0': 'libglib2.0-0t64',
	'libgobject-2.0.so.0': 'libglib2.0-0t64',
	'libgio-2.0.so.0': 'libglib2.0-0t64',
	'libdbus-1.so.3': 'libdbus-1-3',
	'libwebkit2gtk-4.1.so.0': 'libwebkit2gtk-4.1-0',
	'libjavascriptcoregtk-4.1.so.0': 'libjavascriptcoregtk-4.1-0',
	'libsoup-3.0.so.0': 'libsoup-3.0-0',
	'libgcc_s.so.1': 'libgcc-s1',
	'libc.so.6': 'libc6',
};

/** Lo que no enlaza pero hace falta para instalar: el lanzador y el icono. */
const PACKAGING = ['desktop-file-utils', 'hicolor-icon-theme'];

describe('las dependencias del .deb', () => {
	test('declaran cada biblioteca que enlaza el binario', () => {
		const missing = [...new Set(Object.values(LINKED))].filter((pkg) => !depends.includes(pkg));
		expect(missing).toEqual([]);
	});

	test('y nada más que eso y lo del empaquetado', () => {
		const allowed = new Set([...Object.values(LINKED), ...PACKAGING]);
		expect(depends.filter((pkg) => !allowed.has(pkg))).toEqual([]);
	});

	test('sin la libsoup vieja ni repetidos', () => {
		expect(depends).not.toContain('libsoup2.4-1');
		expect(new Set(depends).size).toBe(depends.length);
	});
});
