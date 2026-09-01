class UsersController < ApplicationController
  before_action :require_login, only: [:edit, :update]
  before_action :set_user, only: [:edit, :update]

  def new
    @user = User.new
  end

  def create
    @user = User.new(user_params)
    if @user.save
      session[:user_id] = @user.id
      flash[:notice] = "Account created successfully!"
      redirect_to root_path
    else
      render :new, status: :unprocessable_entity
    end
  end

  def edit
  end

  def update
    if @user.update(user_update_params)
      flash[:notice] = "Profile updated successfully!"
      redirect_to root_path
    else
      render :edit, status: :unprocessable_entity
    end
  end
  
  private
  
  def set_user
    @user = current_user
  end
  
  def user_params
    params.require(:user).permit(:email, :password, :password_confirmation, :first_name, :last_name, :phone, :address)
  end
  
  def user_update_params
    permitted = params.require(:user).permit(:email, :first_name, :last_name, :phone, :address, :password, :password_confirmation)
    permitted.delete(:password) if permitted[:password].blank?
    permitted.delete(:password_confirmation) if permitted[:password_confirmation].blank?
    permitted
  end
end
