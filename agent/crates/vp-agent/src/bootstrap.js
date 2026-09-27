(function () {
  var link = document.getElementById('continue');
  var target = link.getAttribute('data-target');
  var hash = /^#\/pair\?[A-Za-z0-9_\-=&%.]{1,400}$/.test(location.hash) ? location.hash : '';
  link.href = target + hash;

  var status = document.getElementById('status');
  var text = document.getElementById('status-text');
  function check() {
    fetch(target + 'api/info', { mode: 'no-cors', cache: 'no-store' })
      .then(function () {
        status.className = 'status ok';
        text.textContent = 'Cet iPad fait déjà confiance à votre Mac. Vous pouvez continuer.';
      })
      .catch(function () {
        status.className = 'status';
        text.textContent = 'La confiance n’est pas encore activée. Suivez les étapes ci-dessus, puis revenez ici.';
      });
  }
  check();
  document.addEventListener('visibilitychange', function () {
    if (!document.hidden) check();
  });
})();
