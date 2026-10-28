<script setup lang="ts">
import { label } from '../format'

const props = defineProps<{ count: number }>()
</script>

<template>
  <span class="badge">{{ label(props.count) }}</span>
</template>
