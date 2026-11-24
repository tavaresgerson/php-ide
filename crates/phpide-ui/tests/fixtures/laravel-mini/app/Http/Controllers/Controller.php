<?php

declare(strict_types=1);

namespace App\Http\Controllers;

abstract class Controller
{
    /**
     * Renders a template with data (stands in for Laravel's view()).
     *
     * @param array<string, mixed> $data
     */
    protected function render(string $template, array $data = []): string
    {
        return $template . ':' . json_encode($data);
    }
}
