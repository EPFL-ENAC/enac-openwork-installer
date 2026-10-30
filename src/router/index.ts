import { createRouter, createWebHashHistory } from 'vue-router'
import RunView from '../pages/RunView.vue'
import WelcomeScreen from '../pages/steps/WelcomeScreen.vue'

export const routes = [
  { path: '/', name: 'welcome', component: WelcomeScreen },
  { path: '/run', name: 'run', component: RunView },
  { path: '/:pathMatch(.*)*', redirect: '/' },
]

// Hash history: direct navigation to a route works in dev and in the bundled app.
export const router = createRouter({
  history: createWebHashHistory(),
  routes,
})
