import I18n from '@vasakgroup/tauri-plugin-i18n';
import { createPinia } from 'pinia';
import { createApp } from 'vue';
import App from '@/App.vue';
import '@/assets/main.css';

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
			console.error('No se pudieron cargar las traducciones', error);
		}),
	new Promise((resolve) => setTimeout(resolve, PLAZO_TRADUCCIONES_MS)),
]);

app.use(pinia);

app.mount('#app');
