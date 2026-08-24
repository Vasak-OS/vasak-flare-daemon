<script lang="ts" setup>
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from '@vasakgroup/tauri-plugin-i18n';
import { useReactiveIcons } from '@/composables/useReactiveIcon';

const { t } = useI18n();
const appWindow = getCurrentWindow();
const { closeIcon, minimizeIcon, maximizeIcon } = useReactiveIcons({
	closeIcon: 'window-close',
	minimizeIcon: 'window-minimize',
	maximizeIcon: 'window-maximize',
});
</script>
<template>
  <div class="flex gap-1" data-tauri-drag-region>
    <span class="p-1 bg-ui-bg/80 rounded-corner hover:bg-status-success border border-ui-border" @click="appWindow.minimize()" :title="t('window.minimize')">
      <img :src="minimizeIcon" class="h-6 w-6 inline-block" :alt="t('window.minimize')">
    </span>
    <span class="p-1 bg-ui-bg/80 rounded-corner hover:bg-status-warning border border-ui-border" @click="appWindow.toggleMaximize()" :title="t('window.maximize')">
      <img :src="maximizeIcon" class="h-6 w-6 inline-block" :alt="t('window.maximize')">
    </span>
    <span class="p-1 bg-ui-bg/80 rounded-corner hover:bg-status-error border border-ui-border" @click="appWindow.close()" :title="t('window.close')">
      <img :src="closeIcon" class="h-6 w-6 inline-block" :alt="t('window.close')">
    </span>
  </div>
</template>