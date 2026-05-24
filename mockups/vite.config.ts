import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// mdd mockups app — serves high-fidelity React renders of Salt mockups at /mockup/<slug>.
export default defineConfig({
  plugins: [react()],
  server: { port: 4317, strictPort: true },
  preview: { port: 4317, strictPort: true },
})
