<?php

declare(strict_types=1);

namespace App\Http\Controllers;

use App\Models\User;

class UserController extends Controller
{
    public function show(int $id): string
    {
        $user = User::find($id);
        return view('users.show', ['user' => $user]);
    }

    public function index(): string
    {
        $users = User::all();
        return $this->render('users.index', ['users' => $users]);
    }
}

function view(string $template, array $data = []): string
{
    return $template . ':' . count($data);
}
