<script setup lang="ts">
/**
 * Settings → Routing (P12): how OpenRouter's endpoints rank a model for opencode's tiers, and the
 * settings behind that ranking. The parts below are independent; this file only connects them.
 */
import { ref } from 'vue'
import RoutingInspect from './RoutingInspect.vue'
import RoutingObservation from './RoutingObservation.vue'
import RoutingRanking from './RoutingRanking.vue'
import { INTRO } from './routingInspect'

// Bumped when the ranking settings change, so the inspector ranks again with them (and the
// observation reads its status again; it also reloads itself when a check is due).
const inspectVersion = ref(0)
</script>

<template>
  <section class="routing-settings">
    <h2>Routing</h2>
    <p class="intro">{{ INTRO }}</p>
    <RoutingInspect :version="inspectVersion" />
    <RoutingRanking @saved="inspectVersion++" />
    <RoutingObservation :version="inspectVersion" />
  </section>
</template>

<style scoped>
.routing-settings {
  display: flex;
  flex-direction: column;
  gap: 28px;
}
h2 {
  margin: 0 0 6px;
  font-size: 17px;
}
.intro {
  margin: 0;
  color: var(--text-2);
  font-size: 13px;
  max-width: 70ch;
}
</style>
