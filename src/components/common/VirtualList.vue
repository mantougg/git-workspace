<template>
  <div ref="container" class="virtual-list" @scroll.passive="onScroll">
    <div class="virtual-list-spacer" :style="{ height: totalHeight + 'px' }">
      <div
        class="virtual-list-window"
        :style="{ transform: 'translateY(' + offsetY + 'px)' }"
      >
        <div
          v-for="entry in visibleEntries"
          :key="entry.index"
          class="virtual-list-row"
          :style="{ height: itemHeight + 'px' }"
        >
          <slot name="row" :item="entry.item" :index="entry.index" />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts" generic="T">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";

/**
 * Fixed-row-height virtual list (T-04 frontend rendering budget).
 * Bounds the on-screen DOM node count regardless of payload size; rows must
 * be exactly `itemHeight` tall (consumers set `height` + `overflow: hidden`).
 */
const props = defineProps<{
  items: T[];
  itemHeight: number;
}>();

defineSlots<{
  row(props: { item: T; index: number }): unknown;
}>();

/** Extra rows rendered above/below the viewport to avoid blank flashes. */
const OVERSCAN = 10;

const container = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

const totalHeight = computed(() => props.items.length * props.itemHeight);

const startIndex = computed(() =>
  Math.max(0, Math.floor(scrollTop.value / props.itemHeight) - OVERSCAN),
);
const endIndex = computed(() =>
  Math.min(
    props.items.length,
    Math.ceil((scrollTop.value + viewportHeight.value) / props.itemHeight) +
      OVERSCAN,
  ),
);

const visibleEntries = computed(() => {
  const out: Array<{ item: T; index: number }> = [];
  for (let i = startIndex.value; i < endIndex.value; i++) {
    out.push({ item: props.items[i], index: i });
  }
  return out;
});

const offsetY = computed(() => startIndex.value * props.itemHeight);

function onScroll(e: Event) {
  scrollTop.value = (e.target as HTMLElement).scrollTop;
}

let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  const el = container.value;
  if (!el) return;
  viewportHeight.value = el.clientHeight;
  resizeObserver = new ResizeObserver(() => {
    viewportHeight.value = el.clientHeight;
  });
  resizeObserver.observe(el);
});

onUnmounted(() => {
  resizeObserver?.disconnect();
});

// New payload (e.g. another file selected): back to the top.
watch(
  () => props.items,
  () => {
    scrollTop.value = 0;
    if (container.value) container.value.scrollTop = 0;
  },
);
</script>

<style scoped>
.virtual-list {
  height: 100%;
  overflow: auto;
}

/* Grow with the widest row so long lines scroll horizontally. */
.virtual-list-spacer {
  width: max-content;
  min-width: 100%;
}

.virtual-list-window {
  width: max-content;
  min-width: 100%;
}

.virtual-list-row {
  overflow: hidden;
}
</style>
