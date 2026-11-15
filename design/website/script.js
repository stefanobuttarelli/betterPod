/* Only the native sharing recordings need script; the rest of the prototype is HTML/CSS. */
(() => {
  const demo = document.querySelector('#transfer-demo');
  const button = document.querySelector('#transfer-play');
  const status = document.querySelector('#transfer-status');
  const videos = [...demo.querySelectorAll('video')];
  const stills = [...demo.querySelectorAll('.transfer-still')];
  const readyDescriptions = stills.map(image => image.alt);
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
  let completed = false;
  let starting = false;

  function pause() {
    videos.forEach(video => video.pause());
    demo.dataset.state = 'paused';
    button.textContent = 'Continue transfer';
    status.textContent = 'Demo paused';
  }

  function reset() {
    videos.forEach(video => { video.pause(); video.currentTime = 0; });
    stills.forEach((image, index) => { image.src = image.dataset.ready; image.alt = readyDescriptions[index]; });
    completed = false;
    demo.dataset.state = 'ready';
    button.textContent = reduced.matches ? 'Show received album' : 'Play transfer';
    status.textContent = reduced.matches
      ? 'Reduced motion · view the received album without animation'
      : 'Native app animation · demo transfer';
  }

  button.hidden = false;
  reset();
  button.addEventListener('click', async () => {
    if (starting) return;
    if (reduced.matches) {
      completed = !completed;
      stills.forEach((image, index) => {
        image.src = completed ? image.dataset.complete : image.dataset.ready;
        image.alt = completed
          ? (index === 0 ? 'In Rainbows sent successfully from the first console' : 'In Rainbows received on the second console, with Listen Now and Receive Another actions')
          : readyDescriptions[index];
      });
      button.textContent = completed ? 'Reset demo' : 'Show received album';
      status.textContent = completed ? 'Album received · reduced-motion preview' : 'Ready to share · reduced-motion preview';
      return;
    }
    if (videos.some(video => !video.paused)) { pause(); return; }
    if (completed) videos.forEach(video => { video.currentTime = 0; });
    starting = true;
    button.textContent = 'Loading preview…';
    button.setAttribute('aria-busy', 'true');
    try {
      await Promise.all(videos.map(video => video.play()));
      completed = false;
      demo.dataset.state = 'playing';
      button.textContent = 'Pause transfer';
      status.textContent = 'Demo playing · In Rainbows, Radiohead';
    } catch {
      videos.forEach(video => video.pause());
      button.textContent = 'Try preview again';
      status.textContent = 'The recording could not play. The app screens remain visible.';
    } finally {
      starting = false;
      button.removeAttribute('aria-busy');
    }
  });
  videos.forEach(video => video.addEventListener('ended', () => {
    if (!videos.every(item => item.ended)) return;
    completed = true;
    demo.dataset.state = 'complete';
    button.textContent = 'Replay transfer';
    status.textContent = 'Album received · demo complete';
  }));
  reduced.addEventListener('change', reset);
  document.addEventListener('visibilitychange', () => {
    if (document.hidden && videos.some(video => !video.paused)) pause();
  });
  new IntersectionObserver(entries => {
    if (!entries[0].isIntersecting && videos.some(video => !video.paused)) pause();
  }).observe(demo);
})();
