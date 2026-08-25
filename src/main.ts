import { invoke } from '@tauri-apps/api/core';
import I18n from '@vasakgroup/tauri-plugin-i18n';
import { createPinia } from 'pinia';
import { createApp } from 'vue';
import App from '@/App.vue';
import '@/assets/main.css';

/**
 * Saca de una URL lo que no debería quedar en un registro.
 *
 * Se conserva el esquema y la autoridad completos usando `href`, y no
 * `origin + pathname`: para esquemas propios como `asset:` o `ipc:` el `origin`
 * es la cadena «null», así que esa forma escribía `null/ruta` y perdía
 * justamente lo que permite entender qué se bloqueó.
 */
const sanearUrl = (valor: string | null | undefined): string => {
	if (!valor) {
		return '(en línea)';
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
		// No era una URL absoluta —'inline', 'eval', una ruta relativa—: tal cual.
		return valor;
	}
};

document.addEventListener('securitypolicyviolation', (evento) => {
	// Se sanean **las dos** URLs. `sourceFile` también puede llevar query con
	// datos sensibles, y antes se escribía sin tocar.
	console.error(
		`[CSP] bloqueado ${sanearUrl(evento.blockedURI)} por la directiva ` +
			`«${evento.violatedDirective}» en ${sanearUrl(evento.sourceFile) || 'documento'}:${evento.lineNumber}`
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
