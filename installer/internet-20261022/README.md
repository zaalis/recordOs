# Vision-Pro — accès Internet, 22 octobre 2026

Site iPad : https://vision-pro.34-73-186-88.sslip.io

La version portable `vision-pro.exe` de ce dossier a été lancée sur le PC avec le relais Internet configuré. `Vision-Pro_0.1.0_x64-setup.exe` permet d'installer cette version ; cet installateur n'a pas été exécuté dans l'intervention. Les exécutables ne sont pas signés.

Sur le PC, laisser Vision-Pro ouvert. Sur l'iPad, ouvrir le site dans Safari, choisir « Commencer », puis « Autre ordinateur ». Recopier l'Agent ID et « Empreinte pour connexion Internet » depuis « À propos » sur le PC. Choisir ensuite « Connecter un appareil » sur le PC et entrer le code à six chiffres sur l'iPad. Le PC sera mémorisé pour les prochaines connexions, même depuis un autre réseau.

Relais Windows : `vision-pro.34-73-186-88.sslip.io`, mode Réseau → Remote Access. Le PC et l'iPad doivent avoir Internet. L'application sert de tablette graphique/trackpad : elle ne diffuse pas l'écran du PC.

Les réglages et appareils sont conservés dans `%APPDATA%\Vision-Pro`. Ne pas supprimer ce dossier lors d'une mise à jour. Le lancement automatique à l'ouverture de session Windows n'a pas été activé.

SHA-256 :

```text
vision-pro.exe B679C4B1C26C1812065C76F42E83BD94ADD66BC7C01ACC92FB381F4A8A4397F3
Vision-Pro_0.1.0_x64-setup.exe A8E5C48BB538A86D3EBF1B9EBD4F4440C17FD7A64BAABFE24BD2BC3BCD42F942
```

Rapport : `docs/DEPLOYMENT-2026-10-22.md` dans le projet.
