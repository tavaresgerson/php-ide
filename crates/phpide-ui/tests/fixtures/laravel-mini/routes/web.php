<?php

declare(strict_types=1);

use App\Http\Controllers\UserController;

$routes = [
    '/users' => [UserController::class, 'index'],
    '/users/{id}' => [UserController::class, 'show'],
];

return $routes;
