import { invoke } from '@tauri-apps/api/core';
import I18n from '@vasakgroup/tauri-plugin-i18n';
import { createPinia } from 'pinia';
import { createApp } from 'vue';
import App from '@/App.vue';
import '@/assets/main.css';

// Una violación de CSP no se ve: el recurso simplemente no carga y la interfaz
// queda a medias sin decir nada. Esto la manda a la consola, que es donde se
// puede encontrar al ajustar la política.
document.addEventListener('securitypolicyviolation', (evento) => {
	console.error(
		`[CSP] bloqueado ${evento.blockedURI || '(en línea)'} por la directiva ` +
			`«${evento.violatedDirective}» en ${evento.sourceFile ?? 'documento'}:${evento.lineNumber}`
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
