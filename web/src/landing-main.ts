import './app.css';
import LandingApp from './LandingApp.svelte';
import { mount } from 'svelte';

const app = mount(LandingApp, { target: document.getElementById('app')! });

export default app;
