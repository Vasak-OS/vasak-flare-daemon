import { invoke } from '@tauri-apps/api/core';
import I18n from '@vasakgroup/tauri-plugin-i18n';
import { createPinia } from 'pinia';
import { createApp } from 'vue';
import App from '@/App.vue';
import '@/assets/main.css';

/**
 * Los valores que la especificación de CSP informa en lugar de una URL.
 *
 * Van tal cual: no son rutas y recortarlos los volvería ilegibles.
 */
const MARCADORES_CSP = new Set([
	'inline',
	'eval',
	'wasm-eval',
	'data',
	'blob',
	'filesystem',
	'self',
	'unsafe-eval',
	'unsafe-inline',
]);

/**
 * Saca de una URL lo que no debería quedar en un registro.
 *
 * Se conserva el esquema y la autoridad usando `href`, y no `origin + pathname`:
 * para esquemas propios como `asset:` o `ipc:` el `origin` es la cadena «null»,
 * así que esa forma escribía `null/ruta` y perdía justamente lo que permite
 * entender qué se bloqueó.
 *
 * El caso que faltaba cubrir es el del `catch`: una ruta relativa o
 * protocol-relative hace que `new URL` falle, y devolverla tal cual dejaba la
 * query y el fragmento en el registro — o sea, exactamente lo que esta función
 * viene a evitar. Ahora sólo pasan sin tocar los marcadores de la
 * especificación; cualquier otra cosa se corta antes de `?` o `#`.
 */
const sanearUrl = (valor: string | null | undefined): string => {
	if (!valor) {
		return '';
	}
	try {
		const url = new URL(valor);
		if (url.protocol === 'data:') {
			return 'data:(recortado)';
		}
		// Credenciales, query y fragmento: ahí es donde viajan los tokens.
		url.username = '';
		url.password = '';
		url.search = '';
		url.hash = '';
		return url.href;
	} catch {
		if (MARCADORES_CSP.has(valor)) {
			return valor;
		}
		return valor.split(/[?#]/)[0];
	}
};

// Una violación de CSP no se ve: el recurso no carga y la interfaz queda a
// medias sin decir nada. Se sanean **las dos** URLs, porque `sourceFile` también
// puede llevar query con datos sensibles.
document.addEventListener('securitypolicyviolation', (evento) => {
	// El respaldo se decide antes de sanear: `sanearUrl` nunca devuelve vacío
	// para una entrada con contenido, así que un `|| 'documento'` después de
	// llamarla era código muerto.
	const recurso = evento.blockedURI ? sanearUrl(evento.blockedURI) : '(en línea)';
	const origen = evento.sourceFile ? sanearUrl(evento.sourceFile) : 'documento';
	console.error(
		`[CSP] bloqueado ${recurso} por la directiva ` +
			`«${evento.violatedDirective}» en ${origen}:${evento.lineNumber}`
	);
});

/// Un fallo de JavaScript acá deja la aplicación sin montar y el cartel sin
/// aparecer, y la consola del webview no va a ningún archivo. Se reenvía al
/// registro del demonio para que el silencio no sea la única señal.
const avisar = (que: string) => {
	void invoke('trace_js', { message: que }).catch(() => {});
};

window.addEventListener('error', (evento) => {
	avisar(`error: ${evento.message} (${evento.filename}:${evento.lineno})`);
});
window.addEventListener('unhandledrejection', (evento) => {
	avisar(`promesa rechazada: ${String(evento.reason)}`);
});

const app = createApp(App);
const pinia = createPinia();

// Se esperan las traducciones antes de dibujar: los carteles aparecen y se van
// en cinco segundos, así que un cartel con las claves crudas no se corrige
// después, se ve así y listo.
//
// Pero con un plazo, y esto no es defensa hipotética: esta espera está en el
// tope del módulo, así que mientras no resuelva **la aplicación no monta** —no
// hay `onMounted`, no se escuchan los eventos de notificación y el cartel no
// aparece nunca—. Un idioma que tarda es un cartel con las claves crudas;
// un idioma que se cuelga era un escritorio sin notificaciones.
const PLAZO_TRADUCCIONES_MS = 1500;

await Promise.race([
	I18n.getInstance()
		.load()
		.catch((error) => {
			avisar(`no se pudieron cargar las traducciones: ${String(error)}`);
		}),
	new Promise((resolve) => setTimeout(resolve, PLAZO_TRADUCCIONES_MS)),
]);

app.use(pinia);

app.config.errorHandler = (error, _instancia, info) => {
	avisar(`Vue falló en ${info}: ${String(error)}`);
};

app.mount('#app');
