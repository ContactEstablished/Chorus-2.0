<script setup lang="ts">
/** A rail destination that a later phase builds: says which phase and what it will hold. */
import Icon from '../components/Icon.vue'
import type { IconName } from '../components/icons'

defineProps<{ title: string; icon: IconName; phase: string; what: string }>()
</script>

<template>
  <section class="placeholder">
    <h1>{{ title }}</h1>
    <div class="card">
      <Icon :name="icon" :size="28" />
      <p class="phase">Arrives in {{ phase }}</p>
      <p class="what">{{ what }}</p>
    </div>
  </section>
</template>

<style scoped>
.placeholder {
  padding: 28px 40px;
}
h1 {
  margin: 0 0 22px;
  font-size: 28px;
}
.card {
  max-width: 520px;
  padding: 28px;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-card);
  color: var(--text-2);
}
.card > .icon {
  color: var(--text-muted);
}
.phase {
  margin: 14px 0 6px;
  color: var(--text);
  font-weight: 600;
}
.what {
  margin: 0;
  line-height: 1.5;
}
</style>
