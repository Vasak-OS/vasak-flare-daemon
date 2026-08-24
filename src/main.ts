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
await I18n.getInstance().load();

app.use(pinia);

app.mount('#app');
