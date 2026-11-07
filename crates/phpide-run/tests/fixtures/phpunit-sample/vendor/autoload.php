<?php
// Autoloader mínimo do fixture (o phar do PHPUnit carrega a si mesmo;
// este arquivo existe para exercitar a consolidação de bootstrap).
$bs = chr(92);
spl_autoload_register(function (string $class) use ($bs): void {
    $map = [
        'App' . $bs . 'Tests' . $bs => __DIR__ . '/../tests/',
        'App' . $bs => __DIR__ . '/../src/',
    ];
    foreach ($map as $prefix => $dir) {
        if (str_starts_with($class, $prefix)) {
            $file = $dir . str_replace($bs, '/', substr($class, strlen($prefix))) . '.php';
            if (is_file($file)) {
                require $file;
            }
            return;
        }
    }
});
