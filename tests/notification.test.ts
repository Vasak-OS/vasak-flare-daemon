import { describe, expect, test } from 'bun:test';
import {
	DEFAULT_ACTION,
	type FlareNotification,
	GENERIC_APP_ICON,
	hasDefaultAction,
	iconFor,
	parseActions,
} from '../src/tools/notification';

function notification(overrides: Partial<FlareNotification> = {}): FlareNotification {
	return {
		id: 1,
		notif_id: 11,
		app_name: 'Correo',
		app_icon: 'mail-unread',
		summary: 'Ana Pérez',
		body: '',
		urgency: 1,
		created_at: 0,
		read: false,
		...overrides,
	};
}

/** Lo que haría `convertFileSrc`, para ver qué ruta le llega. */
const asset = (path: string) => `asset://localhost${path}`;

describe('las acciones', () => {
	test('se separan en clave y texto, sin la que dispara el clic', () => {
		const actions = parseActions(
			notification({ actions: ['default', 'Abrir', 'archive', 'Archivar', 'reply', 'Responder'] })
		);
		expect(actions).toEqual([
			{ key: 'archive', label: 'Archivar' },
			{ key: 'reply', label: 'Responder' },
		]);
	});

	test('una acción sin texto se muestra con su clave', () => {
		expect(parseActions(notification({ actions: ['snooze', ''] }))).toEqual([
			{ key: 'snooze', label: 'snooze' },
		]);
	});

	test('sin acciones no hay botones', () => {
		expect(parseActions(notification())).toEqual([]);
	});

	test('la acción por omisión se busca entre las claves y no entre los textos', () => {
		expect(hasDefaultAction(notification({ actions: [DEFAULT_ACTION, 'Abrir'] }))).toBe(true);
		// «default» como texto de otra acción no es la acción por omisión.
		expect(hasDefaultAction(notification({ actions: ['open', 'default'] }))).toBe(false);
		expect(hasDefaultAction(notification())).toBe(false);
	});
});

describe('el icono', () => {
	test('un nombre va al tema, con un respaldo genérico', () => {
		expect(iconFor('mail-unread', asset)).toEqual({
			name: 'mail-unread',
			fallbacks: [GENERIC_APP_ICON],
			src: '',
		});
	});

	test('una ruta absoluta va por el protocolo de assets, no como file://', () => {
		expect(iconFor('/usr/share/pixmaps/app.png', asset)).toEqual({
			name: '',
			fallbacks: [],
			src: 'asset://localhost/usr/share/pixmaps/app.png',
		});
		expect(iconFor('file:///home/ana/icono.png', asset)?.src).toBe('asset://localhost/home/ana/icono.png');
	});

	test('una URI file:// se decodifica antes de pasar por el protocolo de assets', () => {
		// Sin decodificar, `convertFileSrc` codificaba el `%20` otra vez y el
		// archivo no se encontraba.
		expect(iconFor('file:///home/ana/mi%20icono.png', asset)?.src).toBe(
			'asset://localhost/home/ana/mi icono.png'
		);
		expect(iconFor('file://localhost/usr/share/icono.png', asset)?.src).toBe(
			'asset://localhost/usr/share/icono.png'
		);
	});

	test('una URI de otra máquina o mal formada no da icono', () => {
		expect(iconFor('file://otra-maquina/home/ana/icono.png', asset)).toBeNull();
		expect(iconFor('file:///home/ana/%E0%A4%A.png', asset)).toBeNull();
	});

	test('sin icono no se reserva el lugar', () => {
		// Como antes: un cartel sin icono arranca el texto contra el borde.
		expect(iconFor('', asset)).toBeNull();
	});
});
