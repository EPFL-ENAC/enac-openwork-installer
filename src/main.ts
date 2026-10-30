import { createApp } from 'vue'
import { Quasar } from 'quasar'
import 'quasar/src/css/index.sass'
import './css/epfl-tokens.css'
import './css/epfl.css'
import './css/epfl-quasar-brand.css'
import App from './App.vue'
import { i18n } from './i18n'
import { router } from './router'

createApp(App).use(Quasar).use(i18n).use(router).mount('#app')
