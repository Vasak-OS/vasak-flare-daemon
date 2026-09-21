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
 *
 * Con el mismo molde vino `useReactiveIcon()`, y con el mismo final: noventa y
 * una líneas que **nadie llamaba nunca**. Lo mismo pero peor, porque un
 * composable muerto no se ve raro: se lee como una pieza de la casa, y el
 * primero que necesite un icono lo va a usar en vez de `ThemeIcon`, que es lo
 * que hizo que hubiera una copia distinta en cada repositorio.
 */

import { describe, expect, test } from 'bun:test';
import { Glob } from 'bun';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

// `fileURLToPath` y no `.pathname`: éste deja los caracteres codificados tal
// como están, así que una ruta con un espacio llega como `%20` y `scanSync` no
// encuentra nada. Y una guardia que no encuentra archivos pasa: las tres
// comprobaciones se cumplen sobre una lista vacía. De ahí también la cuarta.
const raiz = fileURLToPath(new URL('../src/', import.meta.url));

// `.ts` además de `.vue`: `getCurrentWindow().close()` se escribe igual de bien
// en un servicio, y `layouts/` podría volver con un archivo que no sea un
// componente.
const fuentes = await Promise.all(
	[...new Glob('**/*.{vue,ts}').scanSync(raiz)].map(
		async (ruta) => [ruta, await Bun.file(join(raiz, ruta)).text()] as const
	)
);
const rutas = fuentes.map(([ruta]) => ruta);

describe('los carteles', () => {
	test('hay algo que mirar', () => {
		// Sin esto las tres de abajo pasan con la lista vacía, que es en lo que
		// quedan si el patrón deja de encontrar archivos. Una guardia que se
		// apaga sola es peor que no tenerla: dice que sí.
		expect(rutas).toContain('App.vue');
		expect(rutas.length).toBeGreaterThan(3);
	});

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

	test('y tampoco el composable de iconos que vino con el molde', () => {
		// Noventa y una líneas que nadie llamaba. Lo que lo hace peligroso no es
		// que ocupe lugar: es que está disponible. `ThemeIcon` de la librería
		// hace lo mismo con un solo oyente del cambio de tema para toda la
		// ventana; el composable ponía uno por instancia.
		const copia = rutas.filter((ruta) => ruta.includes('useReactiveIcon'));

		expect(copia).toEqual([]);
	});

	test('y nadie resuelve iconos del tema por su cuenta en un componente', () => {
		// La forma de la copia, no su nombre: pedirle la ruta al complemento y
		// volver a pedírsela cuando cambia el tema.
		//
		// `App.vue` es la excepción y es de fondo: el icono de una notificación
		// lo manda quien la envía, y puede ser una **ruta absoluta** o un
		// `file://` de una aplicación cualquiera. `ThemeIcon` sólo entiende
		// nombres del tema, así que ahí `resolveIcon` hace algo que el
		// componente compartido no puede hacer.
		const culpables = fuentes
			.filter(([ruta, texto]) => ruta !== 'App.vue' && /vicons:theme-changed/.test(texto))
			.map(([ruta]) => ruta);

		expect(culpables).toEqual([]);
	});

	test('y no queda el molde de una aplicación con ventana', () => {
		// Lo que se borró. Sin esta comprobación vuelve en la próxima copia de
		// plantilla y nadie lo nota, porque no se usa: se queda ahí ocupando
		// lugar y pidiendo que alguien lo «arregle».
		// Filtrando la lista y no con un glob de `**/`: en Bun ese comodín pide
		// al menos un directorio, así que `layouts/**/*.vue` no encuentra
		// `layouts/WindowAppLayout.vue` y la guardia pasaba con el molde
		// puesto. Se vio devolviendo los archivos a su sitio.
		const molde = rutas.filter(
			(ruta) => ruta.startsWith('layouts/') || ruta.startsWith('components/topbar/')
		);

		expect(molde).toEqual([]);
	});
});
