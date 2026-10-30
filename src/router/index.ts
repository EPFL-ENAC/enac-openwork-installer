import { createRouter, createWebHashHistory } from 'vue-router'
import NetworkCheckScreen from '../pages/steps/NetworkCheckScreen.vue'
import OpencodeScreen from '../pages/steps/OpencodeScreen.vue'
import OpenworkScreen from '../pages/steps/OpenworkScreen.vue'
import ProviderConfigScreen from '../pages/steps/ProviderConfigScreen.vue'
import RcpKeyScreen from '../pages/steps/RcpKeyScreen.vue'
import SmokeTestScreen from '../pages/steps/SmokeTestScreen.vue'
import SummaryScreen from '../pages/steps/SummaryScreen.vue'
import WelcomeScreen from '../pages/steps/WelcomeScreen.vue'
import type { StepId } from '../lib/flow'

export const routes = [
  { path: '/', name: 'welcome', component: WelcomeScreen },
  { path: '/network', name: 'network', component: NetworkCheckScreen },
  { path: '/opencode', name: 'opencode', component: OpencodeScreen },
  { path: '/rcp-key', name: 'rcpKey', component: RcpKeyScreen },
  { path: '/provider-config', name: 'providerConfig', component: ProviderConfigScreen },
  { path: '/smoke-test', name: 'smokeTest', component: SmokeTestScreen },
  { path: '/openwork', name: 'openwork', component: OpenworkScreen },
  { path: '/summary', name: 'summary', component: SummaryScreen },
  { path: '/:pathMatch(.*)*', redirect: '/' },
]

// Hash history: direct navigation to a route works in dev and in the bundled app.
export const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export const isStepRoute = (name: unknown): name is StepId =>
  typeof name === 'string' && routes.some((route) => route.name === name)
