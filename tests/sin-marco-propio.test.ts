/**
 * Este demonio no dibuja ninguna ventana.
 *
 * Lo que muestra son carteles de notificación: una ventana layer-shell sin
 * decoración, anclada a una esquina, que se estira según cuántos haya. No tiene
 * borde de ventana, ni esquina redondeada de ventana, ni barra, ni botones de
 * minimizar y cerrar.
 *
 * Igual nació con el molde de una aplicación normal puesto —`WindowAppLayout` y
 * un `TopBarComponent` con los tres controles— que no importaba nadie y sólo se
 * importaba a sí mismo. Se borró. Esto es para que no vuelva: es justo el tipo
 * de archivo que alguien copia «porque está en todos los repositorios» y
 * termina dibujando una barra de título arriba de un cartel.
 */

import { describe, expect, test } from 'bun:test';
import { Glob } from 'bun';

const raiz = new URL('../src/', import.meta.url).pathname;
const componentes = [...new Glob('**/*.vue').scanSync(raiz)];
const fuentes = await Promise.all(
	componentes.map(async (ruta) => [ruta, await Bun.file(raiz + ruta).text()] as const)
);

describe('los carteles', () => {
	test('no dibujan marco de ventana', () => {
		// `rounded-corner-window` es la esquina de **la ventana**, y sale del
		// marco compartido. Un cartel lleva `rounded-corner` a secas.
		const conMarco = fuentes
			.filter(([, texto]) => texto.includes('rounded-corner-window'))
			.map(([ruta]) => ruta);

		expect(conMarco).toEqual([]);
	});

	test('no llevan controles de ventana', () => {
		// `getCurrentWindow().minimize()` y compañía: no hay nada que
		// minimizar ni maximizar, y cerrar el webview de los carteles apagaría
		// las notificaciones hasta el próximo arranque.
		const conControles = fuentes
			.filter(([, texto]) => /\.(minimize|toggleMaximize)\(/.test(texto))
			.map(([ruta]) => ruta);

		expect(conControles).toEqual([]);
	});

	test('y no queda el molde de una aplicación con ventana', () => {
		// Lo que se borró. Sin esta comprobación vuelve en la próxima copia de
		// plantilla y nadie lo nota, porque no se usa: se queda ahí ocupando
		// lugar y pidiendo que alguien lo «arregle».
		const molde = [...new Glob('{layouts,components/topbar}/**/*.vue').scanSync(raiz)];

		expect(molde).toEqual([]);
	});
});
