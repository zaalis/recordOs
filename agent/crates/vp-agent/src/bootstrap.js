(function () {
  var link = document.getElementById('continue');
  var target = link.getAttribute('data-target');
  var status = document.getElementById('status');
  var text = document.getElementById('status-text');
  var steps = document.getElementById('steps');
  var leaving = false;

  // The secure app only loads once this iPad trusts the PC's certificate: as soon as it does,
  // continue there on its own (the code shown on the PC is entered in the app).
  function check() {
    if (leaving) return;
    fetch(target + 'api/info', { mode: 'no-cors', cache: 'no-store' })
      .then(function () {
        leaving = true;
        status.className = 'status ok';
        text.textContent = 'Connexion sécurisée prête. Ouverture de Record OS…';
        location.replace(target);
      })
      .catch(function () {
        status.className = 'status';
        steps.hidden = false;
        text.textContent = 'Première connexion : installez le profil de sécurité (étapes ci-dessus), puis revenez ici.';
      });
  }
  check();
  document.addEventListener('visibilitychange', function () {
    if (!document.hidden) check();
  });
  window.setInterval(check, 4000);
})();
